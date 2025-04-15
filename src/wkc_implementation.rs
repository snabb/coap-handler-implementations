//! Implementation of a /.well-known/core resource
//!
//! This is a private module to retain flexibilty around the implementation (even though all types
//! need to be public as they are associated types).

use crate::wkc::write_link_format;
use coap_handler::{Handler, Reporting};
use coap_message::{
    error::RenderableOnMinimal, Code as _, MessageOption, MinimalWritableMessage,
    MutableWritableMessage, ReadableMessage,
};
use coap_message_utils::{option_value::Block2RequestData, OptionsExt};
use core::fmt::Debug;

const LINK_FORMAT: u16 = 40;

/// Wrapper around a reporting handler that makes all reported resources discoverable at the path
/// `/.well-known/core`.
///
/// This can be constructed via [crate::ReportingHandlerBuilder::with_wkc()], typically after
/// having gathered all resource in one handler.
pub struct WellKnownCore<H: Reporting + Handler>(H);

impl<H: Reporting + Handler> WellKnownCore<H> {
    pub(crate) fn new(handler: H) -> Self {
        WellKnownCore(handler)
    }
}

pub enum WkcData<T> {
    Wkc {
        // easiest error indication...
        code: u8,
        // to be extended later to queries, content format and more
        block2: Block2RequestData,
    },
    Other(T),
}

/// Error indicating that something went wrong rendering the response to a [WellKnownCore] handler
/// -- either in then handler itself, or in the tree it was set on top of.
#[derive(Debug)]
pub enum WkcBuildResponseError<IE: RenderableOnMinimal + Debug, HBRE: RenderableOnMinimal> {
    Wkc(IE),
    Other(HBRE),
}

impl<IE: RenderableOnMinimal + Debug, HBRE: RenderableOnMinimal + Debug> RenderableOnMinimal
    for WkcBuildResponseError<IE, HBRE>
{
    type Error<IE2: RenderableOnMinimal + Debug> = WkcBuildResponseError<
        <IE as RenderableOnMinimal>::Error<IE2>,
        <HBRE as RenderableOnMinimal>::Error<IE2>,
    >;
    fn render<M: MinimalWritableMessage>(
        self,
        message: &mut M,
    ) -> Result<(), Self::Error<M::UnionError>> {
        match self {
            WkcBuildResponseError::Wkc(e) => e.render(message).map_err(WkcBuildResponseError::Wkc),
            WkcBuildResponseError::Other(e) => {
                e.render(message).map_err(WkcBuildResponseError::Other)
            }
        }
    }
}

impl<H: Reporting + Handler> Handler for WellKnownCore<H> {
    type RequestData = WkcData<H::RequestData>;
    type ExtractRequestError = H::ExtractRequestError;
    type BuildResponseError<M: MinimalWritableMessage> =
        WkcBuildResponseError<M::UnionError, H::BuildResponseError<M>>;

    fn extract_request_data<M: ReadableMessage>(
        &mut self,
        req: &M,
    ) -> Result<Self::RequestData, Self::ExtractRequestError> {
        let mut block2 = None;

        let mut pathmatch = 0;

        let mut accept: Option<u16> = None;

        let opts = req
            .options()
            .ignore_uri_host()
            .filter(|o| match o.number() {
                coap_numbers::option::ACCEPT if accept.is_none() => {
                    accept = o.value_uint();
                    // leave option around to fail later if conversion failed
                    accept.is_none()
                }
                _ => true,
            })
            // Because the resource definition of RFC6690 explicitly allows ignoring these arguments
            .ignore_uri_query()
            .take_block2(&mut block2)
            // Don't take this as template, this is quick & dirty
            .take_uri_path(|p| {
                pathmatch = match (pathmatch, p) {
                    (0, ".well-known") => 1,
                    (1, "core") => 2,
                    _ => -1,
                }
            });

        let remaining = opts.ignore_elective_others();

        let block2 = block2.unwrap_or_default();

        if pathmatch == 2 {
            let code;
            if remaining.is_err() {
                code = coap_numbers::code::BAD_OPTION;
            } else if accept.unwrap_or(LINK_FORMAT) != LINK_FORMAT {
                code = coap_numbers::code::NOT_ACCEPTABLE;
            } else {
                code = coap_numbers::code::CONTENT;
            }
            Ok(WkcData::Wkc { code, block2 })
        } else {
            Ok(WkcData::Other(self.0.extract_request_data(req)?))
        }
    }
    fn estimate_length(&mut self, req: &Self::RequestData) -> usize {
        match req {
            // FIXME precision, consider requester block size
            WkcData::Wkc { .. } => 1024,
            WkcData::Other(req) => self.0.estimate_length(req),
        }
    }
    fn build_response<M: MutableWritableMessage>(
        &mut self,
        m: &mut M,
        req: Self::RequestData,
    ) -> Result<(), Self::BuildResponseError<M>> {
        match req {
            // FIXME extend own BuildResponseError type to also allow raising from here
            WkcData::Wkc { code, block2 } => {
                m.set_code(M::Code::new(code).map_err(|e| WkcBuildResponseError::Wkc(e.into()))?);
                if code == coap_numbers::code::CONTENT {
                    crate::helpers::block2_write_with_cf(
                        block2,
                        m,
                        |w| {
                            write_link_format(w, &self.0, &[]).expect("Block writers do not err.");
                        },
                        Some(40),
                    );
                } else {
                    m.truncate(0)
                        .map_err(|e| WkcBuildResponseError::Wkc(e.into()))?;
                }
                Ok(())
            }
            WkcData::Other(req) => self
                .0
                .build_response(m, req)
                .map_err(WkcBuildResponseError::Other),
        }
    }
}

/// For reporting, a [WellKnownCore] handler simply passes on its inner description; the exposed
/// .well-known/core resource is not listed.
///
/// Utilizing this (when in having a WellKnownCore handler around the resources, then adding more
/// resources using [`HandlerBuilder::at()`](crate::HandlerBuilder::at()), and wrapping the result in a WellKnownCore again) is
/// wasteful in terms of computation (as the inner one will never be hit), but can be convenient
/// for composition in examples.
impl<H: Reporting + Handler> Reporting for WellKnownCore<H> {
    type Record<'res>
        = H::Record<'res>
    where
        H: 'res;
    type Reporter<'res>
        = H::Reporter<'res>
    where
        H: 'res;

    fn report(&self) -> Self::Reporter<'_> {
        self.0.report()
    }
}
