//! Module containing the [TypeHandler] handler and the [TypeRenderable]
//! trait, along with the [TypeSerializer] helper trait and its corresponding implementations
//! for various serde(ish) (de)serializers.

use crate::helpers::{block2_write_with_cf, Block2RequestData};
use crate::option_processing::OptionsExt;
use crate::Error;
use coap_handler::Handler;
use coap_message::{
    Code as _, MessageOption, MinimalWritableMessage, MutableWritableMessage, ReadableMessage,
};
use coap_numbers::{code, option};
use core::marker::PhantomData;

use serde::Serialize;

/// A simple Handler trait that supports GET, POST and/or PUT on a data structure that supports
/// serde.
///
/// A TypeRenderable implementation can be turned into a [Handler] by wrapping it in
/// [TypeHandler::new].
pub trait TypeRenderable {
    type Get;
    type Post;
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

    pub struct SerdeCBORSerialization;
    pub struct MiniCBORSerialization;
}
use sealed::*;

impl TypeSerializer for SerdeCBORSerialization {
    const CF: Option<u16> = coap_numbers::content_format::from_str("application/cbor");
}
impl TypeSerializer for MiniCBORSerialization {
    const CF: Option<u16> = coap_numbers::content_format::from_str("application/cbor");
}

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
pub struct TypeHandler<H, S: TypeSerializer = SerdeCBORSerialization>
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

impl<H> TypeHandler<H, SerdeCBORSerialization>
where
    H: TypeRenderable,
    H::Get: for<'de> serde::Serialize,
    H::Post: for<'de> serde::Deserialize<'de>,
    H::Put: for<'de> serde::Deserialize<'de>,
{
    pub fn new(handler: H) -> Self {
        TypeHandler {
            handler,
            _phantom: PhantomData,
        }
    }
}

impl<H> TypeHandler<H, MiniCBORSerialization>
where
    H: TypeRenderable,
    H::Get: for<'de> minicbor::Encode<()>,
    H::Post: for<'de> minicbor::Decode<'de, ()>,
    H::Put: for<'de> minicbor::Decode<'de, ()>,
{
    pub fn new_minicbor(handler: H) -> Self {
        TypeHandler {
            handler,
            _phantom: PhantomData,
        }
    }
}

/// Data carried around between a request and its response for [TypeHandler]s
pub struct TypeRequestData(TypeRequestDataE);

enum TypeRequestDataE {
    Get(Block2RequestData), // GET to be processed later, but all request opions were in order
    Done(u8), // All done, just a response to emit -- if POST/PUT has been processed, or GET had a bad accept/option
}
use self::TypeRequestDataE::{Done, Get};

// FIXME for all the below: deduplicate (but not sure how, without HKTs)

impl<H> Handler for TypeHandler<H, SerdeCBORSerialization>
where
    H: TypeRenderable,
    H::Get: for<'de> serde::Serialize,
    H::Post: for<'de> serde::Deserialize<'de>,
    H::Put: for<'de> serde::Deserialize<'de>,
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
                Self::check_postput_options(request)?;

                // FIXME: allow getting a mutable payload here, which may be hard for general
                // Message but usually cheap for GNRC-based.
                let parsed: H::Post =
                    serde_cbor::de::from_slice_with_scratch(request.payload(), &mut [])
                        .map_err(|_| Error::bad_request())?;
                Done(self.handler.post(&parsed))
            }
            code::PUT => {
                Self::check_postput_options(request)?;

                let parsed: H::Put =
                    serde_cbor::de::from_slice_with_scratch(request.payload(), &mut [])
                        .map_err(|_| Error::bad_request())?;
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
        Ok(match request.0 {
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
                            |win| repr.serialize(&mut serde_cbor::ser::Serializer::new(win)),
                            SerdeCBORSerialization::CF,
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
        })
    }
}

impl<H> Handler for TypeHandler<H, MiniCBORSerialization>
where
    H: TypeRenderable,
    H::Get: for<'de> minicbor::Encode<()>,
    H::Post: for<'de> minicbor::Decode<'de, ()>,
    H::Put: for<'de> minicbor::Decode<'de, ()>,
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
                Self::check_postput_options(request)?;

                // FIXME: allow getting a mutable payload here, which may be hard for general
                // Message but usually cheap for GNRC-based.
                let parsed: H::Post =
                    minicbor::decode(request.payload()).map_err(|_| Error::bad_request())?;
                Done(self.handler.post(&parsed))
            }
            code::PUT => {
                Self::check_postput_options(request)?;

                let parsed: H::Put =
                    minicbor::decode(request.payload()).map_err(|_| Error::bad_request())?;
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
        Ok(match request.0 {
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
                            |win| minicbor::encode(&repr, win),
                            MiniCBORSerialization::CF,
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
        })
    }
}
