//! Module containing the [SimpleRendered] handler and the [SimpleRenderable] trait its users need.

use crate::helpers::block2_write_with_cf;
use crate::{Error, wkc};
use coap_handler::{Handler, Reporting};
use coap_message::{
    Code as _, MessageOption, MinimalWritableMessage, MutableWritableMessage, ReadableMessage,
};
use coap_message_utils::option_value::Block2RequestData;
use coap_numbers::code::{CONTENT, GET};
use coap_numbers::option::{ACCEPT, BLOCK2, Criticality, get_criticality};

/// Information a SimpleRenderable needs to carry from request to response.
// Newtype wrapper to avoid exposing Block2RequestData
pub struct SimpleRenderableData(Block2RequestData);

/// A simplified Handler trait that can react to GET requests and will render to a fmt::Write
/// object with blockwise backing.
///
/// Anything that implements it (which includes plain &str, for example) can be packed into a
/// [SimpleRendered] to form a Handler.
pub trait SimpleRenderable {
    fn render<W: embedded_io::blocking::Write + core::fmt::Write>(&mut self, writer: &mut W);

    /// If something is returned, GETs with differing Accept options will be rejecected, and the
    /// value will be set in responses.
    fn content_format(&self) -> Option<u16> {
        None
    }
}

/// A container that turns any [SimpleRenderable] (including [&str]) into a CoAP resource
/// [Handler].
///
/// See [SimpleRenderable] for further information.
#[derive(Debug, Copy, Clone)]
pub struct SimpleRendered<T: SimpleRenderable>(pub T);

impl<'a> SimpleRendered<TypedStaticRenderable<'a>> {
    pub fn new_typed_slice(data: &'a [u8], content_format: Option<u16>) -> Self {
        SimpleRendered(TypedStaticRenderable {
            data,
            content_format,
        })
    }

    pub fn new_typed_str(data: &'a str, content_format: Option<u16>) -> Self {
        let data = data.as_bytes();
        Self::new_typed_slice(data, content_format)
    }
}

impl<T> Handler for SimpleRendered<T>
where
    T: SimpleRenderable,
{
    type RequestData = SimpleRenderableData;
    type ExtractRequestError = Error;
    type BuildResponseError<M: MinimalWritableMessage> = M::UnionError;

    fn extract_request_data<M: ReadableMessage>(
        &mut self,
        request: &M,
    ) -> Result<Self::RequestData, Error> {
        let expected_accept = self.0.content_format();

        let mut block2 = None;

        for o in request.options() {
            match o.number() {
                ACCEPT => {
                    if expected_accept.is_some() && o.value_uint() != expected_accept {
                        return Err(Error::bad_option(ACCEPT));
                    }
                }
                BLOCK2 => {
                    block2 = match block2 {
                        Some(_) => return Err(Error::bad_request()),
                        None => Block2RequestData::from_option(&o)
                            .map(Some)
                            // Unprocessed CoAP Option is also for "is aware but could not process"
                            .map_err(|_| Error::bad_option(BLOCK2))?,
                    }
                }
                o if get_criticality(o) == Criticality::Critical => {
                    return Err(Error::bad_option(o));
                }
                _ => (),
            }
        }

        let reqdata = match request.code().into() {
            GET => block2.unwrap_or_default(),
            _ => return Err(Error::method_not_allowed()),
        };
        Ok(SimpleRenderableData(reqdata))
    }

    fn estimate_length(&mut self, _request: &Self::RequestData) -> usize {
        1280 - 40 - 4 // does this correclty calculate the IPv6 minimum MTU?
    }

    fn build_response<M: MutableWritableMessage>(
        &mut self,
        response: &mut M,
        request: Self::RequestData,
    ) -> Result<(), Self::BuildResponseError<M>> {
        let cf = self.0.content_format();
        let block2data = request.0;
        response.set_code(M::Code::new(CONTENT)?);
        block2_write_with_cf(block2data, response, |w| self.0.render(w), cf);

        Ok(())
    }
}

impl<T> Reporting for SimpleRendered<T>
where
    T: SimpleRenderable,
{
    type Record<'a>
        = wkc::EmptyRecord
    where
        Self: 'a;
    type Reporter<'a>
        = core::iter::Once<wkc::EmptyRecord>
    where
        Self: 'a;

    fn report(&self) -> Self::Reporter<'_> {
        // Using a ConstantSliceRecord instead would be tempting, but that'd need a const return
        // value from self.0.content_format()
        core::iter::once(wkc::EmptyRecord {})
    }
}

impl SimpleRenderable for &str {
    fn render<W>(&mut self, writer: &mut W)
    where
        W: core::fmt::Write,
    {
        writer
            .write_str(self)
            .expect("The backend of SimpleRenderable supports infallible writing");
    }

    fn content_format(&self) -> Option<u16> {
        coap_numbers::content_format::from_str("text/plain; charset=utf-8")
    }
}

pub struct TypedStaticRenderable<'a> {
    data: &'a [u8],
    content_format: Option<u16>,
}

impl SimpleRenderable for TypedStaticRenderable<'_> {
    fn render<W: embedded_io::blocking::Write + core::fmt::Write>(&mut self, writer: &mut W) {
        writer.write_all(self.data).unwrap();
    }

    fn content_format(&self) -> Option<u16> {
        self.content_format
    }
}
