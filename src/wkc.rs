//! Tools to implement a .well-known/core resource easily

use coap_handler::Handler;
use coap_message::{MinimalWritableMessage, MutableWritableMessage, ReadableMessage};

use coap_handler::{Attribute, Record, Reporting};

/// Wrapper around arbitrary Handlers to make them not report in .well-known/core.
///
/// This helps integrating handler implementations that do not yet also implement Reporting by
/// providing a blank report.
///
/// (With specialization, this could be moved into the default implementation).
pub struct NotReporting<H: Handler>(H);

impl<H: Handler> NotReporting<H> {
    pub fn new(handler: H) -> Self {
        NotReporting(handler)
    }
}

// Is DerefMut appropriate? Would be less code...
impl<H: Handler> Handler for NotReporting<H> {
    type RequestData = H::RequestData;
    type ExtractRequestError = H::ExtractRequestError;
    type BuildResponseError<M: MinimalWritableMessage> = H::BuildResponseError<M>;
    fn extract_request_data<M: ReadableMessage>(
        &mut self,
        m: &M,
    ) -> Result<H::RequestData, Self::ExtractRequestError> {
        self.0.extract_request_data(m)
    }
    fn estimate_length(&mut self, r: &Self::RequestData) -> usize {
        self.0.estimate_length(r)
    }
    fn build_response<M: MutableWritableMessage>(
        &mut self,
        m: &mut M,
        r: Self::RequestData,
    ) -> Result<(), Self::BuildResponseError<M>> {
        self.0.build_response(m, r)
    }
}

/// A report with no path, relation or attributes
///
/// This is convenient both in places where a type is needed as an element for an Empty iterator
/// (as in NotReporting) and to wrap a resource with no further properties in (in a Once iterator,
/// relying on the tree builder to augment the empty path with the actual path).
pub struct EmptyRecord;

impl Record for EmptyRecord {
    type PathElement = &'static &'static str; // could just as well be !
    type PathElements = core::iter::Empty<&'static &'static str>;
    type Attributes = core::iter::Empty<Attribute>;

    fn path(&self) -> Self::PathElements {
        core::iter::empty()
    }
    fn rel(&self) -> Option<&str> {
        None
    }
    fn attributes(&self) -> Self::Attributes {
        core::iter::empty()
    }
}

impl<H: Handler> Reporting for NotReporting<H> {
    type Record<'a>
        = EmptyRecord
    where
        Self: 'a;
    type Reporter<'a>
        = core::iter::Empty<EmptyRecord>
    where
        Self: 'a;

    fn report(&self) -> Self::Reporter<'_> {
        core::iter::empty()
    }
}

/// Wrapper around arbitrary Handlers to make them report some attributes. The path is often
/// configured empty, relying on the tree builder to augment it with the actual path.
///
/// This helps integrating handler implementations that do not manually implement Reporting.
///
/// Any Reporting the handler might implement is overridden by this.
pub struct ConstantSingleRecordReport<'a, H: Handler> {
    handler: H,
    path: &'a [&'a str],
    attributes: &'a [Attribute],
}

impl<'a, H: Handler> ConstantSingleRecordReport<'a, H> {
    /// Wrap a handler with attributes reported at its single path
    pub fn new(handler: H, attributes: &'a [Attribute]) -> Self {
        ConstantSingleRecordReport {
            handler,
            path: &[],
            attributes,
        }
    }

    /// Wrap a handler with attributes and additional path components
    ///
    /// Compared to [`.new()`](ConstantSingleRecordReport::new), this is primarily useful for
    /// handlers that *do* perform own path processing (and are thus instanciated `.below()` some
    /// path), but whose root resource is not the resource below which is put. This is a typical
    /// case: To use relative references, it is convenient to make a handler go
    /// `.below(&["path"])`, but then expect an extra `/` segment (an empty component in the path
    /// list) at the end -- and then deal out relative references like `1` or `./2` to go to
    /// `/path/2`. Such handlers put `&[""]` in the path argument.
    pub fn new_with_path(handler: H, attributes: &'a [Attribute], path: &'a [&'a str]) -> Self {
        ConstantSingleRecordReport {
            handler,
            path,
            attributes,
        }
    }
}

// Is DerefMut appropriate? Would be less code...
impl<H: Handler> Handler for ConstantSingleRecordReport<'_, H> {
    type RequestData = H::RequestData;
    type ExtractRequestError = H::ExtractRequestError;
    type BuildResponseError<M: MinimalWritableMessage> = H::BuildResponseError<M>;
    fn extract_request_data<M: ReadableMessage>(
        &mut self,
        m: &M,
    ) -> Result<H::RequestData, Self::ExtractRequestError> {
        self.handler.extract_request_data(m)
    }
    fn estimate_length(&mut self, r: &Self::RequestData) -> usize {
        self.handler.estimate_length(r)
    }
    fn build_response<M: MutableWritableMessage>(
        &mut self,
        m: &mut M,
        r: Self::RequestData,
    ) -> Result<(), Self::BuildResponseError<M>> {
        self.handler.build_response(m, r)
    }
}

pub struct ConstantSliceRecord<'a> {
    path: &'a [&'a str],
    attributes: &'a [Attribute],
}

impl<'a> Record for ConstantSliceRecord<'a> {
    type PathElement = &'a &'a str;
    type PathElements = core::slice::Iter<'a, &'a str>;
    type Attributes = core::iter::Cloned<core::slice::Iter<'a, Attribute>>;

    fn path(&self) -> Self::PathElements {
        self.path.iter()
    }
    fn rel(&self) -> Option<&str> {
        None
    }
    fn attributes(&self) -> Self::Attributes {
        self.attributes.iter().cloned()
    }
}

impl<H: Handler> Reporting for ConstantSingleRecordReport<'_, H> {
    type Record<'b>
        = ConstantSliceRecord<'b>
    where
        Self: 'b;
    type Reporter<'b>
        = core::iter::Once<ConstantSliceRecord<'b>>
    where
        Self: 'b;

    fn report(&self) -> Self::Reporter<'_> {
        core::iter::once(ConstantSliceRecord {
            path: self.path,
            attributes: self.attributes,
        })
    }
}

/// Write a Report into a text write in application/link-format.
///
/// As the reports that applications typically get their hands on (as they implement it themselves)
/// don't have the components to make them into an absolute path, but link-format requires using
/// absolute paths, this takes a prefix argument into which the caller has to feed information
/// about where on the CoAP server the resources are actually located.
///
/// Resources that don't ask that information at construction time (it is gone at run time) can not
/// produce meaningful link-format links to their own resources; they could produce output in HTML
/// or CoRAL, but there are currently no Reporting renderers for those.
///
/// ```
/// use coap_handler_implementations::{wkc::write_link_format, *};
/// let mut out = String::new();
/// write_link_format(&mut out,
///     &new_dispatcher()
///         .at(&["name"], SimpleRendered::new_typed_str("Demo program", Some(0)))
///         .at(&["version"], SimpleRendered::new_typed_str("0.8.15", Some(0))),
///     &["dev"]
///     ).unwrap();
/// assert_eq!(out, "</dev/name>,</dev/version>");
/// ```
pub fn write_link_format(
    w: &mut impl core::fmt::Write,
    report: &impl Reporting,
    prefix: &[&str],
) -> core::fmt::Result {
    let mut first = true;
    for record in report.report() {
        if !first {
            write!(w, ",")?;
        } else {
            first = false;
        }
        write!(w, "<")?;
        for p in prefix.iter() {
            write!(w, "/{p}")?;
        }
        for p in record.path() {
            write!(w, "/{}", p.as_ref())?;
        }
        write!(w, ">")?;
        if let Some(rel) = record.rel() {
            write!(w, ";rel=\"{rel}\"")?;
        }
        for attr in record.attributes() {
            attr.write_link_format(w)?;
        }
    }
    report.write_extra_link_format(w, &mut first)?;
    Ok(())
}
