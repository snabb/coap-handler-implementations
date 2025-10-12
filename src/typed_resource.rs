//! Module containing the [TypeHandler] handler and the [TypeRenderable]
//! trait, along with the [TypeSerializer] helper trait and its corresponding implementations
//! for various serde(ish) (de)serializers.

use crate::Error;
use crate::helpers::block2_write_with_cf;
use coap_handler::Handler;
use coap_message::{
    Code as _, MessageOption, MinimalWritableMessage, MutableWritableMessage, ReadableMessage,
    error::RenderableOnMinimal as _,
};
use coap_message_utils::{OptionsExt, option_value::Block2RequestData};
use coap_numbers::{code, option};
use core::marker::PhantomData;

/// A type that (for minicbor typed handlers) represents an empty payload.
///
/// This is particularly practical for resources that have a GET and PUT handler that act on
/// serialized data, but where the POST handler just takes or return an empty payload.
pub struct Empty;

/// A simple Handler trait that supports various CoAP methods on a data structure that supports
/// different serializations.
///
/// A TypeRenderable implementation can be turned into a [Handler] by wrapping it in
/// [`TypeHandler::new_*`][TypeHandler].
///
/// For different methods, the functions get called at different times in the request/response
/// handling sequence: A GET's method is called when the response is prepared, whereas a PUT's
/// method is called when the request is prepared. This ensures that for CoAP server
/// implementations that delay rendering (e.g. as part of retransmission handling), no large data
/// needs to be kept for long. The methods with both input and output data (POST, FETCH) are called
/// according to their most expected usage pattern: [`Self::fetch()`] is called at response time
/// (because the request payload is typically a small filter expression that limits the output to
/// something that fits within a CoAP response but may still be large), whereas [`Self::patch()`]
/// is called immediately (because it is frequently used with empty outputs).
pub trait TypeRenderable {
    /// Output type of the [get()][Self::get()] method, serialized into the response payload.
    type Get;
    /// Input type of the [post()][Self::post()] method, deserialized from the request payload.
    type PostIn;
    /// Ouptut type of the [post()][Self::post()] method, serialized into the response payload.
    ///
    /// Note that this is a size sensitive type: On CoAP servers that need to retain state between
    /// processing a request and sending a response, this type goes into that state.
    type PostOut;
    /// Input type of the [put()][Self::put()] method, deserialized from the request payload.
    type Put;
    /// Input type of the [fetch()][Self::fetch()] method, deserialized from the request payload.
    ///
    /// Note that this is a size sensitive type: On CoAP servers that need to retain state between
    /// processing a request and sending a response, this type goes into that state.
    type FetchIn;
    /// Ouptut type of the [fetch()][Self::fetch()] method, serialized into the response payload.
    type FetchOut;
    /// Input type of the [ipatch()][Self::ipatch()] method, deserialized from the request payload.
    type IPatch;

    fn get(&mut self) -> Result<Self::Get, Error> {
        Err(Error::method_not_allowed())
    }
    fn post(&mut self, _representation: &Self::PostIn) -> Result<Self::PostOut, Error> {
        Err(Error::method_not_allowed())
    }
    fn put(&mut self, _representation: &Self::Put) -> Result<(), Error> {
        Err(Error::method_not_allowed())
    }
    fn delete(&mut self) -> Result<(), Error> {
        Err(Error::method_not_allowed())
    }
    fn fetch(&mut self, _representation: &Self::FetchIn) -> Result<Self::FetchOut, Error> {
        Err(Error::method_not_allowed())
    }
    fn ipatch(&mut self, _representation: &Self::IPatch) -> Result<(), Error> {
        Err(Error::method_not_allowed())
    }
}

/// Keeping them hidden to stay flexible; they don't need to be named for their'e either default or
/// their users have aliases.
mod sealed {
    pub trait TypeSerializer {
        const CF: Option<u16>;
    }

    pub struct MiniCBORSerialization2;
}
use sealed::*;

/// Wrapper for resource handlers that are implemented in terms of GETting, POSTing or PUTting
/// objects in CBOR format.
///
/// The wrapper handles all encoding and decoding, options processing (ignoring the critical
/// Uri-Path option under the assumption that that has been already processed by an underlying
/// request router), the corresponding errors and block-wise GETs.
///
/// More complex handlers (eg. implementing additional representations, or processing query
/// aprameters into additional data available to the [TypeRenderable]) can be built by
/// forwarding to this (where any critical but already processed options would need to be masked
/// from the message's option) or taking inspiration from it.
pub struct TypeHandler<H, S: TypeSerializer>
where
    H: TypeRenderable,
{
    handler: H,
    _phantom: PhantomData<S>,
}

impl<H, S> TypeHandler<H, S>
where
    H: TypeRenderable,
    S: TypeSerializer,
{
    fn check_get_options(request: &impl ReadableMessage) -> Result<Block2RequestData, Error> {
        let mut block2 = None;

        request
            .options()
            .take_block2(&mut block2)
            .filter(|o| {
                if o.number() == option::ACCEPT {
                    if let Some(cf) = S::CF {
                        // If they differ, we'll keep the option for later failing
                        o.value_uint() != Some(cf)
                    } else {
                        // We don't know any content format, so we keep the option in the iterator
                        // to fail later
                        true
                    }
                } else {
                    true
                }
            })
            .ignore_elective_others()?;

        Ok(block2.unwrap_or_default())
    }

    fn check_fetch_options(request: &impl ReadableMessage) -> Result<Block2RequestData, Error> {
        // FIXME deduplicage with check_get_options, which is almost identical

        let mut block2 = None;

        request
            .options()
            .take_block2(&mut block2)
            .filter(|o| {
                if o.number() == option::CONTENT_FORMAT {
                    // BIG FIXME: *which* format(s) should we accept? POST compares for our CF but
                    // that's not really true.
                    false
                } else if o.number() == option::ACCEPT {
                    if let Some(cf) = S::CF {
                        // If they differ, we'll keep the option for later failing
                        o.value_uint() != Some(cf)
                    } else {
                        // We don't know any content format, so we keep the option in the iterator
                        // to fail later
                        true
                    }
                } else {
                    true
                }
            })
            .ignore_elective_others()?;

        Ok(block2.unwrap_or_default())
    }

    fn check_delete_options(request: &impl ReadableMessage) -> Result<(), Error> {
        request.options().ignore_elective_others()
    }

    fn check_postput_options(request: &impl ReadableMessage) -> Result<(), Error> {
        // BIG FIXME: Splot out POST handling b/c now that can also have accept and that needs
        // checking

        let mut cf = Ok(());

        request
            .options()
            .filter(|o| {
                if o.number() == option::CONTENT_FORMAT
                    && (S::CF.is_none() || o.value_uint() != S::CF)
                {
                    cf = Err(Error::bad_option(option::CONTENT_FORMAT));
                }
                // It's not a critical option so we don't really have to filter it out
                true
            })
            .ignore_elective_others()?;

        cf
    }
}

/// Data carried around between a request and its response for [TypeHandler]s
///
/// This carries input data from FETCH and output data from POST, under the assumption that FETCH
/// generally have small input representations and POST have large output representations; see
/// [`TypeRenderable`] documentation.
pub struct TypeRequestData<FetchIn, PostOut>(TypeRequestDataE<FetchIn, PostOut>);

enum TypeRequestDataE<FetchIn, PostOut> {
    Get(Block2RequestData), // GET to be processed later, but all request opions were in order
    Fetch(Block2RequestData, FetchIn),
    Post(PostOut),
    // FIXME: Those could really go together with the Error in a single variant, but then it's also
    // convenient for Error to be really only errors -- make Error a type alias for Response<const
    // bool CanBeOk, const bool CanBeError>? And may we just abuse ExtractRequestError to also
    // carry it without any other ill-effect?
    DoneCode(u8), // All done, just a response to emit -- if POST/PUT has been processed, or GET had a bad accept/option
}
use self::TypeRequestDataE::{DoneCode, Fetch, Get, Post};

// FIXME for all the below: deduplicate (but not sure how, without HKTs) -- and some alterations
// have accumulated

trait ToTypedResourceError {
    fn into_general_error(self, total_len: usize) -> Error;
}

impl<'b, C> minicbor_2::decode::Decode<'b, C> for Empty {
    fn decode(
        _d: &mut minicbor_2::decode::Decoder<'b>,
        _ctx: &mut C,
    ) -> Result<Self, minicbor_2::decode::Error> {
        Err(minicbor_2::decode::Error::message("No element expected").at(0))
    }

    fn nil() -> Option<Self> {
        Some(Empty)
    }
}

impl<H> Handler for TypeHandler<H, MiniCBORSerialization2>
where
    H: TypeRenderable,
    H::Get: for<'de> minicbor_2::Encode<()>,
    H::PostIn: for<'de> minicbor_2::Decode<'de, ()>,
    H::PostOut: for<'de> minicbor_2::Encode<()>,
    H::Put: for<'de> minicbor_2::Decode<'de, ()>,
    H::FetchIn: for<'de> minicbor_2::Decode<'de, ()>,
    H::FetchOut: for<'de> minicbor_2::Encode<()>,
    H::IPatch: for<'de> minicbor_2::Decode<'de, ()>,
{
    type RequestData = TypeRequestData<H::FetchIn, H::PostOut>;
    type ExtractRequestError = Error;
    type BuildResponseError<M: MinimalWritableMessage> = M::UnionError;

    fn extract_request_data<M: ReadableMessage>(
        &mut self,
        request: &M,
    ) -> Result<Self::RequestData, Error> {
        Ok(TypeRequestData(match request.code().into() {
            code::DELETE => {
                Self::check_delete_options(request)?;
                self.handler.delete()?;
                DoneCode(code::DELETED)
            }
            code::GET => Get(Self::check_get_options(request)?),
            code::POST => {
                use minicbor_2::decode::Decode;
                Self::check_postput_options(request)?;

                let payload = request.payload();
                match (payload, H::PostIn::nil()) {
                    (b"", Some(nil)) => Post(self.handler.post(&nil)?),
                    (payload, _) => {
                        let parsed: H::PostIn =
                            minicbor_2::decode(payload).map_err(|deserialize_error| {
                                deserialize_error.into_general_error(payload.len())
                            })?;
                        Post(self.handler.post(&parsed)?)
                    }
                }
            }
            code::PUT => {
                Self::check_postput_options(request)?;

                let payload = request.payload();

                let parsed: H::Put = minicbor_2::decode(payload).map_err(|deserialize_error| {
                    deserialize_error.into_general_error(payload.len())
                })?;
                self.handler.put(&parsed)?;
                DoneCode(code::CHANGED)
            }
            code::FETCH => {
                let block_options = Self::check_fetch_options(request)?;

                let payload = request.payload();

                let parsed: H::FetchIn =
                    minicbor_2::decode(payload).map_err(|deserialize_error| {
                        deserialize_error.into_general_error(payload.len())
                    })?;
                Fetch(block_options, parsed)
            }
            // BIG FIXME: This is missing code for IPATCH completely, and POST is not really up to
            // date any more
            _ => return Err(Error::method_not_allowed()),
        }))
    }

    fn estimate_length(&mut self, request: &Self::RequestData) -> usize {
        match &request.0 {
            DoneCode(_) => 4,
            Get(block) => (block.size() + 25).into(), // FIXME: hard-coded copied over from block2_write_with_cf's estimated overhead
            // BIG FIXME: huh?
            _ => 1024,
        }
    }

    fn build_response<M: MutableWritableMessage>(
        &mut self,
        response: &mut M,
        request: Self::RequestData,
    ) -> Result<(), Self::BuildResponseError<M>> {
        match request.0 {
            DoneCode(c) => response.set_code(M::Code::new(c)?),
            Get(block2) => {
                let repr = self.handler.get();
                match repr {
                    Err(e) => e.render(response)?,
                    Ok(repr) => {
                        response.set_code(M::Code::new(code::CONTENT)?);
                        match block2_write_with_cf(
                            block2,
                            response,
                            |win| {
                                minicbor_2::encode(&repr, minicbor_adapters::WriteToEmbeddedIo(win))
                            },
                            MiniCBORSerialization2::CF,
                        ) {
                            Ok(()) => (),
                            Err(_) => {
                                // FIXME: Rewind all the written options
                                response.set_code(M::Code::new(code::INTERNAL_SERVER_ERROR)?);
                            }
                        }
                    }
                }
            }
            Fetch(block2, fetch) => {
                let repr = self.handler.fetch(&fetch);
                match repr {
                    Err(e) => e.render(response)?,
                    Ok(repr) => {
                        response.set_code(M::Code::new(code::CONTENT)?);
                        match block2_write_with_cf(
                            block2,
                            response,
                            |win| {
                                minicbor_2::encode(&repr, minicbor_adapters::WriteToEmbeddedIo(win))
                            },
                            MiniCBORSerialization2::CF,
                        ) {
                            Ok(()) => (),
                            Err(_) => {
                                // FIXME: Rewind all the written options
                                response.set_code(M::Code::new(code::INTERNAL_SERVER_ERROR)?);
                            }
                        }
                    }
                }
            }
            _ => {
                todo!("BIG FIXME");
            }
        };
        Ok(())
    }
}

impl TypeSerializer for MiniCBORSerialization2 {
    // FIXME: Allow differentiation by methods
    const CF: Option<u16> = coap_numbers::content_format::from_str("application/cbor");
}

impl<H> TypeHandler<H, MiniCBORSerialization2>
where
    H: TypeRenderable,
    H::Get: for<'de> minicbor_2::Encode<()>,
    H::PostIn: for<'de> minicbor_2::Decode<'de, ()>,
    H::PostOut: for<'de> minicbor_2::Encode<()>,
    H::Put: for<'de> minicbor_2::Decode<'de, ()>,
    H::FetchIn: for<'de> minicbor_2::Decode<'de, ()>,
    H::FetchOut: for<'de> minicbor_2::Encode<()>,
    H::IPatch: for<'de> minicbor_2::Decode<'de, ()>,
{
    /// Wrap a handler through minicbor 2.x
    pub fn new_minicbor_2(handler: H) -> Self {
        TypeHandler {
            handler,
            _phantom: PhantomData,
        }
    }
}

impl ToTypedResourceError for minicbor_2::decode::Error {
    fn into_general_error(self, total_len: usize) -> Error {
        let mut error = Error::bad_request();
        if let Some(position) = self.position() {
            error = Error::bad_request_with_rbep(position);
        }
        if self.is_end_of_input() {
            // Data is not set, but it's convenient to point that out in the error response just in
            // case something goes wrong with block-wise transfers
            error = Error::bad_request_with_rbep(total_len);
        };
        if self.is_type_mismatch() {
            error = error.with_title("Type mismatch")
        }
        error
    }
}
