//! Tools to easily pull options from a [coap_message::ReadableMessage].
//!
//! This will typically be used by filtering down a message's options, e.g. like this:
//!
//! ```
//! # struct ReqData {
//! #     block2: coap_handler_implementations::helpers::Block2RequestData,
//! # }
//!
//! use coap_message::{MessageOption, ReadableMessage};
//! use crate::coap_handler_implementations::option_processing::OptionsExt;
//! use crate::coap_handler_implementations::Error;
//!
//! fn process_message(req: &impl ReadableMessage) -> Result<ReqData, Error>
//! {
//!     let mut block2 = None;
//!
//!     req.options()
//!            .take_block2(&mut block2)
//!            .filter(|o| {
//!                match o.number() {
//!                    // my own option => my own behavior
//!                    _ => true
//!                }
//!            })
//!            .ignore_elective_others()
//!            ?;
//!
//!     let block2 = block2.unwrap_or_default();
//!     Ok(ReqData { block2 })
//! }
//! ```

// FIXME: This is still not used heavily internally, as it lacks facilities to take single options
// (and the old code would only largely profit if it could tat) -- and that's not provided yet
// because of undecidedness around type_alias_impl_trait.

use crate::helpers::{Block2RequestData, TryFromOption};
use crate::Error;
use coap_message::MessageOption;
use coap_numbers::option;

/// Extensions implemented for any [MessageOption] iterator to enable simple and direct use
///
/// See module level documentation for examples.
// No need to seal this: the `for T` implementation already ensures that nobody implements it.
pub trait OptionsExt<O: MessageOption>: Iterator<Item = O> {
    type IgnoredUriHost: Iterator<Item = O> + Sized;
    /// Remove Uri-Host option from the iterator
    ///
    /// Note that not processing this may be inappropriate for security reasons, especially with
    /// security models that otherwise require DNS rebinding protection.
    fn ignore_uri_host(self) -> Self::IgnoredUriHost;

    type IgnoredUriQuery: Iterator<Item = O> + Sized;
    /// Remove Uri-Query options from the iterator
    ///
    /// Note that this is *not* something that should simply be placed in a handler; it should only
    /// be used if the definition of the resource's interface explicitly allows the implementation
    /// to ignore unknown query parameters.
    fn ignore_uri_query(self) -> Self::IgnoredUriQuery;

    /// Exhaust the iterator, successfully if no critical options are present, or indicating an
    /// error if critical options were not processed before.
    fn ignore_elective_others(self) -> Result<(), Error>;

    type WithoutOption<'a, T: TryFromOption>: Iterator<Item = O>
    where
        Self: 'a,
        T: 'a;
    /// Store the first matching and parsable option into `out`, and return the iterator over the
    /// remaining options.
    ///
    /// Unparsable or duplicate options are simply left in the output and rely on their criticality
    /// to cause an error when eventually `.ignore_elective_others()` is called. When using this
    /// mechanism to parse a non-critical option that should not be ignored on parsing errors, that
    /// option should not implement [`TryFromOption`] on `Self`, but rather on `Result<Self,
    /// ParsingError>`, and `if Some(Err(_)) = out` after the options have been exhausted, the
    /// error needs to be raised.
    fn take_into<'a, T: TryFromOption>(self, out: &'a mut Option<T>) -> Self::WithoutOption<'a, T>
    where
        Self: 'a;

    type WithoutBlock2<'a>: Iterator<Item = O>
    where
        Self: 'a;
    /// Set out to the parsed value of the found Block2 option, and return an iterator over the
    /// remaining options.
    ///
    /// Unparsable or repeated Block2 options are left in the output, leaving the error to show up
    /// in the eventual ignore_elective_others call.
    ///
    /// Note that this is merely a pre-typed version of take_into (and not deprecated yet because
    /// it's a convenient shortcut to spelling out the `None`'s type).
    fn take_block2<'a>(self, out: &'a mut Option<Block2RequestData>) -> Self::WithoutBlock2<'a>
    where
        Self: 'a;

    type WithoutUriPath<F: FnMut(&str)>: Iterator<Item = O>;
    /// Call a function (that typically cranks some path state machine) on every (valid) Uri-Path
    /// option in an iterator, hiding them from further iteration.
    ///
    /// Error handling of the UTF8 decoding is done by not removing invalid options from the
    /// iterator, thus leaving them for an eventual ignore_elective_others.
    fn take_uri_path<F: FnMut(&str)>(self, f: F) -> Self::WithoutUriPath<F>;
}

mod aux {
    //! Helpers for while we don't allow type_alias_impl_trait everywhere
    //!
    //! These should go away once type_alias_impl_trait can be assumed.

    use super::*;

    pub struct TryfromPusher<'a, O: MessageOption, I: Iterator<Item = O>, T: TryFromOption> {
        pub(super) instream: I,
        pub(super) out: &'a mut Option<T>,
    }

    impl<'a, O: MessageOption, I: Iterator<Item = O>, T: TryFromOption> Iterator
        for TryfromPusher<'a, O, I, T>
    {
        type Item = O;

        // Note that this is effectively a copy of the take_block2 implementation in the
        // nontrivial_option_processing case.
        //
        // This can go away once the type_alias_impl_trait feature can be assumed.
        fn next(&mut self) -> Option<O> {
            while let Some(o) = self.instream.next() {
                if self.out.is_none() {
                    if let Some(o) = T::try_from(&o) {
                        *self.out = Some(o);
                        continue;
                    }
                }
                return Some(o);
            }
            None
        }
    }

    pub struct UriPusher<O: MessageOption, I: Iterator<Item = O>, F: FnMut(&str)> {
        pub(super) instream: I,
        pub(super) f: F,
    }

    impl<O: MessageOption, I: Iterator<Item = O>, F: FnMut(&str)> Iterator for UriPusher<O, I, F> {
        type Item = O;

        // Note that this is effectively a copy of the take_uri_path implementation in the
        // nontrivial_option_processing case.
        //
        // This can go away once the type_alias_impl_trait feature can be assumed.
        fn next(&mut self) -> Option<O> {
            while let Some(o) = self.instream.next() {
                if o.number() == option::URI_PATH {
                    if let Ok(s) = core::str::from_utf8(o.value()) {
                        (self.f)(s);
                        continue;
                    }
                }
                return Some(o);
            }
            None
        }
    }
}

impl<T, O> OptionsExt<O> for T
where
    T: Iterator<Item = O>,
    O: MessageOption,
{
    type IgnoredUriHost = core::iter::Filter<Self, fn(&O) -> bool>;

    fn ignore_uri_host(self) -> Self::IgnoredUriHost {
        fn keep_option<O: MessageOption>(o: &O) -> bool {
            o.number() != option::URI_HOST
        }
        self.filter(keep_option)
    }

    type IgnoredUriQuery = core::iter::Filter<Self, fn(&O) -> bool>;
    fn ignore_uri_query(self) -> Self::IgnoredUriQuery {
        fn keep_option<O: MessageOption>(o: &O) -> bool {
            o.number() != option::URI_QUERY
        }
        self.filter(keep_option)
    }

    fn ignore_elective_others(mut self) -> Result<(), Error> {
        match self.find(|o| option::get_criticality(o.number()) == option::Criticality::Critical) {
            Some(o) => Err(Error::bad_option(o.number())),
            None => Ok(()),
        }
    }

    type WithoutOption<'a, T2: 'a + TryFromOption> = aux::TryfromPusher<'a, O, Self, T2> where T: 'a;
    fn take_into<'a, T2: 'a + TryFromOption>(
        self,
        out: &'a mut Option<T2>,
    ) -> Self::WithoutOption<'a, T2>
    where
        T: 'a,
    {
        aux::TryfromPusher {
            instream: self,
            out,
        }
    }

    // See TryfromPusher comments
    #[cfg(not(feature = "nontrivial_option_processing"))]
    type WithoutBlock2<'a> = aux::TryfromPusher<'a, O, Self, Block2RequestData> where T: 'a;
    #[cfg(not(feature = "nontrivial_option_processing"))]
    fn take_block2<'a>(self, out: &'a mut Option<Block2RequestData>) -> Self::WithoutBlock2<'a>
    where
        T: 'a,
    {
        aux::TryfromPusher {
            instream: self,
            out,
        }
    }

    #[cfg(feature = "nontrivial_option_processing")]
    type WithoutBlock2<'a> = impl Iterator<Item = O> + 'a where Self: 'a;
    #[cfg(feature = "nontrivial_option_processing")]
    fn take_block2<'a>(self, out: &'a mut Option<Block2RequestData>) -> Self::WithoutBlock2<'a>
    where
        Self: 'a,
    {
        // Beware that this is copied and generalized in TryfromPusher
        self.filter(move |o| {
            if matches!(o.number(), option::BLOCK2) && out.is_none() {
                if let Ok(o) = Block2RequestData::from_option(o) {
                    *out = Some(o);
                    return false;
                }
            }
            true
        })
    }

    #[cfg(not(feature = "nontrivial_option_processing"))]
    type WithoutUriPath<F: FnMut(&str)> = aux::UriPusher<O, Self, F>;
    #[cfg(not(feature = "nontrivial_option_processing"))]
    fn take_uri_path<F: FnMut(&str)>(self, f: F) -> Self::WithoutUriPath<F> {
        aux::UriPusher { instream: self, f }
    }

    #[cfg(feature = "nontrivial_option_processing")]
    type WithoutUriPath<F: FnMut(&str)> = impl Iterator<Item = O>;
    #[cfg(feature = "nontrivial_option_processing")]
    fn take_uri_path<F: FnMut(&str)>(self, mut f: F) -> Self::WithoutUriPath<F> {
        // Beware that this is copied in UriPusher
        self.filter(move |o| {
            if o.number() == option::URI_PATH {
                if let Ok(s) = core::str::from_utf8(o.value()) {
                    f(s);
                    return false;
                }
            }
            true
        })
    }
}
