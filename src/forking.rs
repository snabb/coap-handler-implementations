//! Module containing code for all the components for building trees of resources

use coap_handler::{Attribute, Handler, Reporting};
use coap_message::{
    MessageOption, MinimalWritableMessage, MutableWritableMessage, ReadableMessage,
    error::RenderableOnMinimal,
};
use coap_numbers::option;

use crate::forking_helpers::{ForkingRecord, PrefixedRecord};
use crate::helpers::{MaskingUriUpToPath, MaskingUriUpToPathN};
use crate::{NeverFound, wkc, wkc_implementation};

/// Backport of <https://github.com/rust-lang/rust/issues/64295>. The contested points about the
/// API do not apply.
trait IterOrderByBackport: Iterator {
    fn cmp_by<I, F>(mut self, other: I, mut cmp: F) -> core::cmp::Ordering
    where
        Self: Sized,
        I: IntoIterator,
        F: FnMut(Self::Item, I::Item) -> core::cmp::Ordering,
    {
        let mut other = other.into_iter();

        loop {
            let x = match self.next() {
                None => {
                    if other.next().is_none() {
                        return core::cmp::Ordering::Equal;
                    } else {
                        return core::cmp::Ordering::Less;
                    }
                }
                Some(val) => val,
            };

            let y = match other.next() {
                None => return core::cmp::Ordering::Greater,
                Some(val) => val,
            };

            match cmp(x, y) {
                core::cmp::Ordering::Equal => (),
                non_eq => return non_eq,
            }
        }
    }
}

impl<T: Iterator> IterOrderByBackport for T {}

/// Start building a tree of sub-resources
///
/// While technically this just returns a handler that returns 4.04 unconditionally, it also
/// implements HandlerBuilder, and thus can be used like this:
///
/// ```
/// use coap_handler_implementations::*;
/// let handler = new_dispatcher()
///     .at(&["dev", "name"], SimpleRendered::new_typed_str("Demo program", Some(0)))
///     .at(&["dev", "version"], SimpleRendered::new_typed_str("0.8.15", Some(0)))
///     .with_wkc()
///     ;
/// ```
#[cfg(not(feature = "leaky_names"))]
pub fn new_dispatcher() -> impl Handler + Reporting {
    wkc::NotReporting::new(NeverFound {})
}
#[cfg(feature = "leaky_names")]
pub fn new_dispatcher() -> wkc::NotReporting<NeverFound> {
    wkc::NotReporting::new(NeverFound {})
}

/// Trait implemented by default on all handlers that lets the user stack them using a builder-like
/// syntax.
///
/// Note that the resulting ForkingRequestData<ForkingRequestData<...>,()> enums that might look
/// wasteful on paper are optimized into the minimum necessary size since
/// <https://github.com/rust-lang/rust/pull/45225>. They are, however, suboptimal when it comes to
/// how many times the options are read.
pub trait HandlerBuilder<'a, OldRD>
where
    Self: Handler + Sized,
{
    /// Divert requests arriving at `path` into the given `handler`.
    ///
    /// The handler will not *not* see the Uri-Path (and Uri-Host, as this builder doesn't do
    /// virtual hosting yet) options any more; see the top-level module documentation on Options
    /// Hiding for rationale.
    ///
    /// If both the previous tree and the new handler are Reporting, so is the result.
    fn at<H>(self, path: &'a [&'a str], handler: H) -> ForkingHandler<'a, H, Self>
    where
        H: Handler + Sized,
    {
        ForkingHandler {
            h1: handler,
            h2: self,
            h1_condition: path,
        }
    }

    /// Divert requests arriving at `path` into the given `handler`, and announce them with the
    /// given attributes in .well-known/core.
    ///
    /// Any reporting the handler would have done is overridden.
    ///
    /// This is a shorthand for `.at(ConstantSingleRecordReport::new(h, attributes))`.
    fn at_with_attributes<H>(
        self,
        path: &'a [&'a str],
        attributes: &'a [Attribute],
        handler: H,
    ) -> ForkingHandler<'a, wkc::ConstantSingleRecordReport<'a, H>, Self>
    where
        H: Handler + Sized,
    {
        ForkingHandler {
            h1: wkc::ConstantSingleRecordReport::new(handler, attributes),
            h2: self,
            h1_condition: path,
        }
    }

    /// Divert requests arriving with an Uri-Path starting with `path` to the given `handler`.
    ///
    /// Only remaining Uri-Path options will be visible to the handler; those expressed in path
    /// (and Uri-Host, see [.at()]) are hidden.
    ///
    /// If both the previous tree and the new handler are Reporting, so is the result.
    fn below<H>(self, path: &'a [&'a str], handler: H) -> ForkingTreeHandler<'a, H, Self> {
        ForkingTreeHandler {
            h1: handler,
            h2: self,
            h1_condition: path,
        }
    }
}

impl<OldRD, OldH> HandlerBuilder<'_, OldRD> for OldH
where
    Self: Handler<RequestData = OldRD> + Sized,
{
    // Methods are provided
}

/// Extension trait for handlers that also implement [Reporting](coap_handler::Reporting).
///
/// Like [HandlerBuilder] this is implemented for wherever it works.
///
/// (Note that while this *could* be implemented as a provided method of Reporting, it is split out
/// to stay architecturally analogous to the HandlerBuilder, and to not force this crate's
/// implementation of .well-known/core onto other users of Reporting. Possibly, these could be
/// separated approaching stabilization.)
pub trait ReportingHandlerBuilder<'a, OldRD>: HandlerBuilder<'a, OldRD> + Reporting {
    /// Add a `/.well-known/core` resource that exposes the information the previous (stack of)
    /// handler(s) exposes throught the [Reporting](coap_handler::Reporting) trait.
    fn with_wkc(self) -> wkc_implementation::WellKnownCore<Self> {
        wkc_implementation::WellKnownCore::new(self)
    }

    fn with_extra_record(
        self,
        target: &'a str,
        attributes: &'a [Attribute],
    ) -> WithExtraRecord<'a, Self> {
        WithExtraRecord::new(self, target, attributes)
    }
}

impl<OldRD, OldH> ReportingHandlerBuilder<'_, OldRD> for OldH
where
    OldH: Handler<RequestData = OldRD> + Reporting,
{
    // Methods are provided
}

pub struct ForkingHandler<'a, H1, H2> {
    h1: H1,
    h2: H2,

    // I'd like to have a closure in here, and that'd almost work as a type D: Fn(&Message<Bin>)
    // -> bool, but I can't write at()... -> ForkingHandler<impl ..., H, Self> in the trait's
    // signature.
    h1_condition: &'a [&'a str],
}

/// Tagged-union container for ForkingHandler
#[derive(Debug)]
pub enum ForkingRequestData<RD1, RD2> {
    First(RD1),
    Second(RD2),
}

impl<RE1, RE2> RenderableOnMinimal for ForkingRequestData<RE1, RE2>
where
    RE1: RenderableOnMinimal + core::fmt::Debug,
    RE2: RenderableOnMinimal + core::fmt::Debug,
{
    type Error<IE: RenderableOnMinimal + core::fmt::Debug> = ForkingRequestData<
        <RE1 as RenderableOnMinimal>::Error<IE>,
        <RE2 as RenderableOnMinimal>::Error<IE>,
    >;

    fn render<M: MinimalWritableMessage>(
        self,
        message: &mut M,
    ) -> Result<(), Self::Error<M::UnionError>> {
        use ForkingRequestData::*;
        match self {
            First(e) => e.render(message).map_err(First),
            Second(e) => e.render(message).map_err(Second),
        }
    }
}

impl<RD1, H1, RD2, H2> Handler for ForkingHandler<'_, H1, H2>
where
    H1: Handler<RequestData = RD1>,
    H2: Handler<RequestData = RD2>,
{
    type RequestData = ForkingRequestData<RD1, RD2>;

    type ExtractRequestError = ForkingRequestData<H1::ExtractRequestError, H2::ExtractRequestError>;
    type BuildResponseError<M: MinimalWritableMessage> =
        ForkingRequestData<H1::BuildResponseError<M>, H2::BuildResponseError<M>>;

    fn extract_request_data<M: ReadableMessage>(
        &mut self,
        request: &M,
    ) -> Result<Self::RequestData, Self::ExtractRequestError> {
        let expected_path = self.h1_condition.iter().map(|s| s.as_bytes());
        let actual_path = request.options().filter(|o| o.number() == option::URI_PATH);

        Ok(
            if IterOrderByBackport::cmp_by(expected_path, actual_path, |e, a| e.cmp(a.value()))
                == core::cmp::Ordering::Equal
            {
                let masked = MaskingUriUpToPath(request);
                ForkingRequestData::First(
                    self.h1
                        .extract_request_data(&masked)
                        .map_err(ForkingRequestData::First)?,
                )
            } else {
                ForkingRequestData::Second(
                    self.h2
                        .extract_request_data(request)
                        .map_err(ForkingRequestData::Second)?,
                )
            },
        )
    }

    fn estimate_length(&mut self, request: &Self::RequestData) -> usize {
        match request {
            ForkingRequestData::First(r) => self.h1.estimate_length(r),
            ForkingRequestData::Second(r) => self.h2.estimate_length(r),
        }
    }

    fn build_response<M: MutableWritableMessage>(
        &mut self,
        response: &mut M,
        request: Self::RequestData,
    ) -> Result<(), Self::BuildResponseError<M>> {
        match request {
            ForkingRequestData::First(r) => self
                .h1
                .build_response(response, r)
                .map_err(ForkingRequestData::First)?,
            ForkingRequestData::Second(r) => self
                .h2
                .build_response(response, r)
                .map_err(ForkingRequestData::Second)?,
        }
        Ok(())
    }
}

impl<RD1, H1, RD2, H2> Reporting for ForkingHandler<'_, H1, H2>
where
    H1: Handler<RequestData = RD1> + Reporting,
    H2: Handler<RequestData = RD2> + Reporting,
{
    // FIXME: This is copied over from ForkingTreeHandler (and stripped to the general not(feature
    // = "_nontrivial_option_processing") case)
    //
    // As it is now it would appear to warrant the Forking{,Tree}Handler unification hinted at the
    // ForkingTreeHandler definition. Not starting the deduplication (which would be warranted by
    // the below behemoth) as this'll all look much slimmer again once type_alias_impl_trait is
    // on by default.
    //
    // (After that, could be that it's warranted as it's now more than a few 4-liners, could be
    // that not).

    type Record<'b>
        = ForkingRecord<PrefixedRecord<'b, H1::Record<'b>>, H2::Record<'b>>
    where
        Self: 'b;

    type Reporter<'b>
        = core::iter::Chain<
        core::iter::Map<H2::Reporter<'b>, fn(H2::Record<'b>) -> Self::Record<'b>>,
        core::iter::Map<
            core::iter::Zip<H1::Reporter<'b>, core::iter::Repeat<&'b [&'b str]>>,
            fn((H1::Record<'b>, &'b [&'b str])) -> Self::Record<'b>,
        >,
    >
    where
        Self: 'b;
    fn report(&self) -> Self::Reporter<'_> {
        fn first<'c, H1R, H2R>(
            prefixed_prefix: (H1R, &'c [&'c str]),
        ) -> ForkingRecord<PrefixedRecord<'c, H1R>, H2R> {
            let (prefixed, prefix) = prefixed_prefix;
            ForkingRecord::First(PrefixedRecord { prefix, prefixed })
        }
        self.h2
            .report()
            .map(ForkingRecord::Second as fn(_) -> _)
            .chain(
                self.h1
                    .report()
                    .zip(core::iter::repeat(self.h1_condition))
                    .map(first as fn(_) -> _),
            )
    }

    fn write_extra_link_format(
        &self,
        writer: &mut impl core::fmt::Write,
        is_first: &mut bool,
    ) -> core::fmt::Result {
        self.h1.write_extra_link_format(writer, is_first)?;
        self.h2.write_extra_link_format(writer, is_first)?;
        Ok(())
    }
}

// This is identical to the ForkingHandler in its structure -- just the matching behavior differs.
// Even ForkingRequestData can be shared; unfortunately, the main code is still duplicated -- it
// could be refactored, but is it worth it for the identical 4-lines parts?
pub struct ForkingTreeHandler<'a, H1, H2> {
    h1: H1,
    h2: H2,

    h1_condition: &'a [&'a str],
}

impl<RD1, H1, RD2, H2> Handler for ForkingTreeHandler<'_, H1, H2>
where
    H1: Handler<RequestData = RD1>,
    H2: Handler<RequestData = RD2>,
{
    type RequestData = ForkingRequestData<RD1, RD2>;

    type ExtractRequestError = ForkingRequestData<H1::ExtractRequestError, H2::ExtractRequestError>;
    type BuildResponseError<M: MinimalWritableMessage> =
        ForkingRequestData<H1::BuildResponseError<M>, H2::BuildResponseError<M>>;

    fn extract_request_data<M: ReadableMessage>(
        &mut self,
        request: &M,
    ) -> Result<Self::RequestData, Self::ExtractRequestError> {
        use ForkingRequestData::*;

        let expected_path = self.h1_condition.iter().map(|s| s.as_bytes());
        let actual_path = request
            .options()
            .filter(|o| o.number() == option::URI_PATH)
            .take(self.h1_condition.len());

        if IterOrderByBackport::cmp_by(expected_path, actual_path, |e, a| e.cmp(a.value()))
            == core::cmp::Ordering::Equal
        {
            let masked = MaskingUriUpToPathN::new(request, self.h1_condition.len());
            self.h1
                .extract_request_data(&masked)
                .map(First)
                .map_err(First)
        } else {
            self.h2
                .extract_request_data(request)
                .map(Second)
                .map_err(Second)
        }
    }

    fn estimate_length(&mut self, request: &Self::RequestData) -> usize {
        match request {
            ForkingRequestData::First(r) => self.h1.estimate_length(r),
            ForkingRequestData::Second(r) => self.h2.estimate_length(r),
        }
    }

    fn build_response<M: MutableWritableMessage>(
        &mut self,
        response: &mut M,
        request: Self::RequestData,
    ) -> Result<(), Self::BuildResponseError<M>> {
        use ForkingRequestData::*;
        match request {
            First(r) => self.h1.build_response(response, r).map_err(First),
            Second(r) => self.h2.build_response(response, r).map_err(Second),
        }
    }
}

impl<RD1, H1, RD2, H2> Reporting for ForkingTreeHandler<'_, H1, H2>
where
    H1: Handler<RequestData = RD1> + Reporting,
    H2: Handler<RequestData = RD2> + Reporting,
{
    type Record<'b>
        = ForkingRecord<PrefixedRecord<'b, H1::Record<'b>>, H2::Record<'b>>
    where
        Self: 'b;

    // FIXME: one of these is copied over to Reporting for ForkingHandler, see there.

    #[cfg(feature = "_nontrivial_option_processing")]
    type Reporter<'b>
        = impl Iterator<Item = Self::Record<'b>>
    where
        Self: 'b;

    #[cfg(feature = "_nontrivial_option_processing")]
    fn report(&self) -> Self::Reporter<'_> {
        self.h2
            .report()
            .map(ForkingRecord::Second as fn(_) -> _)
            .chain(self.h1.report().map(|f| {
                ForkingRecord::First(PrefixedRecord {
                    prefix: self.h1_condition,
                    prefixed: f,
                })
            }))
    }

    #[cfg(not(feature = "_nontrivial_option_processing"))]
    type Reporter<'b>
        = core::iter::Chain<
        core::iter::Map<H2::Reporter<'b>, fn(H2::Record<'b>) -> Self::Record<'b>>,
        core::iter::Map<
            core::iter::Zip<H1::Reporter<'b>, core::iter::Repeat<&'b [&'b str]>>,
            fn((H1::Record<'b>, &'b [&'b str])) -> Self::Record<'b>,
        >,
    >
    where
        Self: 'b;
    #[cfg(not(feature = "_nontrivial_option_processing"))]
    fn report(&self) -> Self::Reporter<'_> {
        fn first<'c, H1R, H2R>(
            prefixed_prefix: (H1R, &'c [&'c str]),
        ) -> ForkingRecord<PrefixedRecord<'c, H1R>, H2R> {
            let (prefixed, prefix) = prefixed_prefix;
            ForkingRecord::First(PrefixedRecord { prefix, prefixed })
        }
        self.h2
            .report()
            .map(ForkingRecord::Second as fn(_) -> _)
            .chain(
                self.h1
                    .report()
                    .zip(core::iter::repeat(self.h1_condition))
                    .map(first as fn(_) -> _),
            )
    }
}

pub struct WithExtraRecord<'a, H> {
    handler: H,
    target: &'a str,
    attributes: &'a [Attribute],
}

impl<'a, H> WithExtraRecord<'a, H> {
    pub fn new(handler: H, target: &'a str, attributes: &'a [Attribute]) -> Self {
        Self {
            handler,
            target,
            attributes,
        }
    }
}

// Plain forwarding implementation
impl<H: Handler> Handler for WithExtraRecord<'_, H> {
    type RequestData = H::RequestData;
    type ExtractRequestError = H::ExtractRequestError;
    type BuildResponseError<M: MinimalWritableMessage> = H::BuildResponseError<M>;

    fn extract_request_data<M: ReadableMessage>(
        &mut self,
        request: &M,
    ) -> Result<Self::RequestData, Self::ExtractRequestError> {
        self.handler.extract_request_data(request)
    }

    fn estimate_length(&mut self, request: &Self::RequestData) -> usize {
        self.handler.estimate_length(request)
    }

    fn build_response<M: MutableWritableMessage>(
        &mut self,
        response: &mut M,
        request: Self::RequestData,
    ) -> Result<(), Self::BuildResponseError<M>> {
        self.handler.build_response(response, request)
    }
}

impl<'a, H: Reporting> Reporting for WithExtraRecord<'a, H> {
    type Record<'b>
        = H::Record<'b>
    where
        Self: 'b;

    type Reporter<'b>
        = H::Reporter<'b>
    where
        Self: 'b;
    fn report(&self) -> Self::Reporter<'_> {
        self.handler.report()
    }

    fn write_extra_link_format(
        &self,
        writer: &mut impl core::fmt::Write,
        is_first: &mut bool,
    ) -> core::fmt::Result {
        self.handler.write_extra_link_format(writer, is_first)?;

        if !*is_first {
            write!(writer, ",")?;
        } else {
            *is_first = false;
        }

        write!(writer, "<{}>", self.target)?;
        for a in self.attributes {
            a.write_link_format(writer)?;
        }

        Ok(())
    }
}
