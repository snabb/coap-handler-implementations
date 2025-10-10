//! Module containing the [TypeHandler] handler and the [TypeRenderable]
//! trait, along with the [TypeSerializer] helper trait and its corresponding implementations
//! for various serde(ish) (de)serializers.

use crate::Error;
use crate::helpers::block2_write_with_cf;
use coap_handler::Handler;
use coap_message::{
    Code as _, MessageOption, MinimalWritableMessage, MutableWritableMessage, ReadableMessage,
};
use coap_message_utils::{OptionsExt, option_value::Block2RequestData};
use coap_numbers::{code, option};
use core::marker::PhantomData;

/// A type that (for minicbor typed handlers) represents an empty payload.
///
/// This is particularly practical for resources that have a GET and PUT handler that act on
/// serialized data, but where the POST handler just takes an empty resource.
pub struct Empty;

/// A simple Handler trait that supports GET, POST and/or PUT on a data structure that supports
/// different serializations.
///
/// A TypeRenderable implementation can be turned into a [Handler] by wrapping it in
/// [`TypeHandler::new_*`][TypeHandler].
pub trait TypeRenderable {
    /// Output type of the [get()][Self::get()] method, serialized into the response payload.
    type Get;
    /// Input type of the [post()][Self::post()] method, deserialized from the request payload.
    ///
    /// Note that with TypedRenderable in this version, the response is always empty.
    type Post;
    /// Input type of the [put()][Self::put()] method, deserialized from the request payload.
    type Put;

    fn get(&mut self) -> Result<Self::Get, u8> {
        Err(code::METHOD_NOT_ALLOWED)
    }
    fn post(&mut self, _representation: &Self::Post) -> u8 {
        code::METHOD_NOT_ALLOWED
    }
    fn put(&mut self, _representation: &Self::Put) -> u8 {
        code::METHOD_NOT_ALLOWED
    }
    fn delete(&mut self) -> u8 {
        code::METHOD_NOT_ALLOWED
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

    fn check_delete_options(request: &impl ReadableMessage) -> Result<(), Error> {
        request.options().ignore_elective_others()
    }

    fn check_postput_options(request: &impl ReadableMessage) -> Result<(), Error> {
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
pub struct TypeRequestData(TypeRequestDataE);

enum TypeRequestDataE {
    Get(Block2RequestData), // GET to be processed later, but all request opions were in order
    Done(u8), // All done, just a response to emit -- if POST/PUT has been processed, or GET had a bad accept/option
}
use self::TypeRequestDataE::{Done, Get};

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
    H::Post: for<'de> minicbor_2::Decode<'de, ()>,
    H::Put: for<'de> minicbor_2::Decode<'de, ()>,
{
    type RequestData = TypeRequestData;
    type ExtractRequestError = Error;
    type BuildResponseError<M: MinimalWritableMessage> = M::UnionError;

    fn extract_request_data<M: ReadableMessage>(
        &mut self,
        request: &M,
    ) -> Result<Self::RequestData, Error> {
        Ok(TypeRequestData(match request.code().into() {
            code::DELETE => {
                Self::check_delete_options(request)?;
                Done(self.handler.delete())
            }
            code::GET => Get(Self::check_get_options(request)?),
            code::POST => {
                use minicbor_2::decode::Decode;
                Self::check_postput_options(request)?;

                let payload = request.payload();
                match (payload, H::Post::nil()) {
                    (b"", Some(nil)) => Done(self.handler.post(&nil)),
                    (payload, _) => {
                        let parsed: H::Post =
                            minicbor_2::decode(payload).map_err(|deserialize_error| {
                                deserialize_error.into_general_error(payload.len())
                            })?;
                        Done(self.handler.post(&parsed))
                    }
                }
            }
            code::PUT => {
                Self::check_postput_options(request)?;

                let payload = request.payload();

                let parsed: H::Put = minicbor_2::decode(payload).map_err(|deserialize_error| {
                    deserialize_error.into_general_error(payload.len())
                })?;
                Done(self.handler.put(&parsed))
            }
            _ => Done(code::METHOD_NOT_ALLOWED),
        }))
    }

    fn estimate_length(&mut self, request: &Self::RequestData) -> usize {
        match &request.0 {
            Done(_) => 4,
            Get(block) => (block.size() + 25).into(), // FIXME: hard-coded copied over from block2_write_with_cf's estimated overhead
        }
    }

    fn build_response<M: MutableWritableMessage>(
        &mut self,
        response: &mut M,
        request: Self::RequestData,
    ) -> Result<(), Self::BuildResponseError<M>> {
        match request.0 {
            Done(r) => response.set_code(M::Code::new(r)?),
            Get(block2) => {
                let repr = self.handler.get();
                match repr {
                    Err(e) => response.set_code(M::Code::new(e)?),
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
        };
        Ok(())
    }
}

impl TypeSerializer for MiniCBORSerialization2 {
    const CF: Option<u16> = coap_numbers::content_format::from_str("application/cbor");
}

impl<H> TypeHandler<H, MiniCBORSerialization2>
where
    H: TypeRenderable,
    H::Get: for<'de> minicbor_2::Encode<()>,
    H::Post: for<'de> minicbor_2::Decode<'de, ()>,
    H::Put: for<'de> minicbor_2::Decode<'de, ()>,
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
