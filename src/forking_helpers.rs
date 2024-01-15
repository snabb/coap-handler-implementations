//! Types that are necessarily public (because the Forking implementations need to expose suitable
//! associated types) but that are not intended to ever be stabilized.
//!
//! While they do have concrete associated types, these should not be relied on externally; once
//! type_alias_impl_trait can be used, they should all switch to opaque types if just to ease API
//! stability.

use coap_handler::{Attribute, Record};

pub enum ForkingIterator<T, I1: Iterator<Item = T>, I2: Iterator<Item = T>> {
    First(I1),
    Second(I2),
}
impl<T, I1: Iterator<Item = T>, I2: Iterator<Item = T>> Iterator for ForkingIterator<T, I1, I2> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        match self {
            ForkingIterator::First(i) => i.next(),
            ForkingIterator::Second(i) => i.next(),
        }
    }
}

pub enum ForkingAsrefstr<AR1: AsRef<str>, AR2: AsRef<str>> {
    First(AR1),
    Second(AR2),
}
impl<AR1: AsRef<str>, AR2: AsRef<str>> AsRef<str> for ForkingAsrefstr<AR1, AR2> {
    fn as_ref(&self) -> &str {
        match self {
            Self::First(s) => s.as_ref(),
            Self::Second(s) => s.as_ref(),
        }
    }
}
pub enum ForkingAsrefstrIterator<
    AR1: AsRef<str>,
    AR2: AsRef<str>,
    I1: Iterator<Item = AR1>,
    I2: Iterator<Item = AR2>,
> {
    First(I1),
    Second(I2),
}
impl<AR1: AsRef<str>, AR2: AsRef<str>, I1: Iterator<Item = AR1>, I2: Iterator<Item = AR2>> Iterator
    for ForkingAsrefstrIterator<AR1, AR2, I1, I2>
{
    type Item = ForkingAsrefstr<AR1, AR2>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::First(i) => i.next().map(ForkingAsrefstr::First),
            Self::Second(i) => i.next().map(ForkingAsrefstr::Second),
        }
    }
}

pub enum ForkingRecord<H1R, H2R> {
    First(H1R),
    Second(H2R),
}
// FIXME: The rest of Forking is relatively sane (worst part so far is iterating over the path
// options many times), but this has potential to compile into quadratic insanity!
impl<H1R: Record, H2R: Record> Record for ForkingRecord<H1R, H2R> {
    type PathElement = ForkingAsrefstr<H1R::PathElement, H2R::PathElement>;
    type PathElements = ForkingAsrefstrIterator<
        H1R::PathElement,
        H2R::PathElement,
        H1R::PathElements,
        H2R::PathElements,
    >;
    type Attributes = ForkingIterator<Attribute, H1R::Attributes, H2R::Attributes>;

    fn path(&self) -> Self::PathElements {
        match self {
            ForkingRecord::First(h1) => ForkingAsrefstrIterator::First(h1.path()),
            ForkingRecord::Second(h2) => ForkingAsrefstrIterator::Second(h2.path()),
        }
    }
    fn rel(&self) -> Option<&'static str> {
        None
    }
    fn attributes(&self) -> Self::Attributes {
        match self {
            ForkingRecord::First(h1) => ForkingIterator::First(h1.attributes()),
            ForkingRecord::Second(h2) => ForkingIterator::Second(h2.attributes()),
        }
    }
}

pub struct PrefixedRecord<'a, R> {
    pub(crate) prefix: &'a [&'a str],
    pub(crate) prefixed: R,
}

impl<'a, R: Record> Record for PrefixedRecord<'a, R> {
    // Usually we use Forking as "First if one type of request comes in, Second for the other", but
    // here it changes when going through the items.
    type PathElement = ForkingAsrefstr<&'a &'a str, R::PathElement>;
    type PathElements = core::iter::Chain<
        core::iter::Map<core::slice::Iter<'a, &'a str>, fn(&'a &'a str) -> Self::PathElement>,
        core::iter::Map<R::PathElements, fn(R::PathElement) -> Self::PathElement>,
    >;
    type Attributes = R::Attributes;

    fn path(&self) -> Self::PathElements {
        self.prefix
            .iter()
            .map(ForkingAsrefstr::First as fn(_) -> _)
            .chain(
                self.prefixed
                    .path()
                    .map(ForkingAsrefstr::Second as fn(_) -> _),
            )
    }
    fn rel(&self) -> Option<&str> {
        self.prefixed.rel()
    }
    fn attributes(&self) -> Self::Attributes {
        self.prefixed.attributes()
    }
}
