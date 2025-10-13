//! Types that implement TypeRenderable for partial sets of traits.
//!
//! These structs private, and constructed through the up to 62 public methods in
//! [`super::generated`].

// No matter whether we're working with concat_idents to build large names or with a module
// hierachy: So far I'm stuck generating type constraints from a declarative macro -- using the
// limited list above instead.
//
// Instead, code is generated and checked in as `generated.rs`.
//
// macro_rules! generate_wrap {
//     ($wrapper:expr, $($constraints:tt),*) => {
//         pub fn wrap<T>(input: T) -> impl $crate::TypeRenderable
//             where $($constraints)*
//         {
//             $wrapper(input)
//         }
//     }
// }
//
// macro_rules! ipatch {
//     ($wrapper:expr, $($constraints:tt),*) => {
//         mod ipatch {
//             generate_wrap!(|wrapper| { $wrapper(wrapper) }, ($($constraints),*, $crate::typed_resource::IPatchRenderable))
//         }
//         generate_wrap!(|wrapper| { $crate::typed_resource::fillers::FillIPatch($wrapper(wrapper)) }, $($constraints),*)
//     }
// }
//
// mod get {
//     ipatch!(|wrapper| wrapper, (T: crate::typed_resource::GetRenderable));
// }
// ipatch!(
//     |wrapper| crate::typed_resource::fillers::FillGet(wrapper),
//     ()
// );

macro_rules! delegate_get {
    ($type:ident) => {
        impl<T: super::GetRenderable> super::GetRenderable for $type<T> {
            type Get = T::Get;

            #[inline]
            fn get(&mut self) -> Result<Self::Get, coap_message_utils::Error> {
                self.0.get()
            }
        }
    };
}

macro_rules! delegate_post {
    ($type:ident) => {
        impl<T: super::PostRenderable> super::PostRenderable for $type<T> {
            type PostIn = T::PostIn;
            type PostOut = T::PostOut;

            #[inline]
            fn post(
                &mut self,
                representation: &Self::PostIn,
            ) -> Result<Self::PostOut, coap_message_utils::Error> {
                self.0.post(representation)
            }
        }
    };
}

macro_rules! delegate_put {
    ($type:ident) => {
        impl<T: super::PutRenderable> super::PutRenderable for $type<T> {
            type Put = T::Put;

            #[inline]
            fn put(&mut self, representation: &Self::Put) -> Result<(), coap_message_utils::Error> {
                self.0.put(representation)
            }
        }
    };
}

macro_rules! delegate_delete {
    ($type:ident) => {
        impl<T: super::DeleteRenderable> super::DeleteRenderable for $type<T> {
            #[inline]
            fn delete(&mut self) -> Result<(), coap_message_utils::Error> {
                self.0.delete()
            }
        }
    };
}

macro_rules! delegate_fetch {
    ($type:ident) => {
        impl<T: super::FetchRenderable> super::FetchRenderable for $type<T> {
            type FetchIn = T::FetchIn;
            type FetchOut = T::FetchOut;

            #[inline]
            fn fetch(
                &mut self,
                representation: &Self::FetchIn,
            ) -> Result<Self::FetchOut, coap_message_utils::Error> {
                self.0.fetch(representation)
            }
        }
    };
}

macro_rules! delegate_ipatch {
    ($type:ident) => {
        impl<T: super::IPatchRenderable> super::IPatchRenderable for $type<T> {
            type IPatch = T::IPatch;

            #[inline]
            fn ipatch(
                &mut self,
                representation: &Self::IPatch,
            ) -> Result<(), coap_message_utils::Error> {
                self.0.ipatch(representation)
            }
        }
    };
}

pub struct FillGet<T>(pub(crate) T);
impl<T> super::sealed::ExtensibleRenderable for FillGet<T> {}

impl<T> super::GetRenderable for FillGet<T> {
    type Get = ();

    // …and the provided impl is good enough
}

delegate_post!(FillGet);
delegate_put!(FillGet);
delegate_delete!(FillGet);
delegate_fetch!(FillGet);
delegate_ipatch!(FillGet);
impl<
    T: super::PostRenderable
        + super::PutRenderable
        + super::DeleteRenderable
        + super::FetchRenderable
        + super::IPatchRenderable,
> super::TypeRenderable for FillGet<T>
{
}

pub struct FillPost<T>(pub(crate) T);
impl<T> super::sealed::ExtensibleRenderable for FillPost<T> {}

delegate_get!(FillPost);

impl<T> super::PostRenderable for FillPost<T> {
    type PostIn = ();
    type PostOut = ();

    // …and the provided impl is good enough
}

delegate_put!(FillPost);
delegate_delete!(FillPost);
delegate_fetch!(FillPost);
delegate_ipatch!(FillPost);
impl<
    T: super::GetRenderable
        + super::PutRenderable
        + super::DeleteRenderable
        + super::FetchRenderable
        + super::IPatchRenderable,
> super::TypeRenderable for FillPost<T>
{
}

pub struct FillPut<T>(pub(crate) T);
impl<T> super::sealed::ExtensibleRenderable for FillPut<T> {}

delegate_get!(FillPut);
delegate_post!(FillPut);

impl<T> super::PutRenderable for FillPut<T> {
    type Put = ();

    // …and the provided impl is good enough
}

delegate_delete!(FillPut);
delegate_fetch!(FillPut);
delegate_ipatch!(FillPut);
impl<
    T: super::GetRenderable
        + super::PostRenderable
        + super::DeleteRenderable
        + super::FetchRenderable
        + super::IPatchRenderable,
> super::TypeRenderable for FillPut<T>
{
}

pub struct FillDelete<T>(pub(crate) T);
impl<T> super::sealed::ExtensibleRenderable for FillDelete<T> {}

delegate_get!(FillDelete);
delegate_post!(FillDelete);
delegate_put!(FillDelete);

impl<T> super::DeleteRenderable for FillDelete<T> {
    // …and the provided impl is good enough
}

delegate_fetch!(FillDelete);
delegate_ipatch!(FillDelete);
impl<
    T: super::GetRenderable
        + super::PostRenderable
        + super::PutRenderable
        + super::FetchRenderable
        + super::IPatchRenderable,
> super::TypeRenderable for FillDelete<T>
{
}

pub struct FillFetch<T>(pub(crate) T);
impl<T> super::sealed::ExtensibleRenderable for FillFetch<T> {}

delegate_get!(FillFetch);
delegate_post!(FillFetch);
delegate_put!(FillFetch);
delegate_delete!(FillFetch);

impl<T> super::FetchRenderable for FillFetch<T> {
    type FetchIn = ();
    type FetchOut = ();

    // …and the provided impl is good enough
}

delegate_ipatch!(FillFetch);
impl<
    T: super::GetRenderable
        + super::PostRenderable
        + super::PutRenderable
        + super::DeleteRenderable
        + super::IPatchRenderable,
> super::TypeRenderable for FillFetch<T>
{
}

pub struct FillIPatch<T>(pub(crate) T);
impl<T> super::sealed::ExtensibleRenderable for FillIPatch<T> {}

delegate_get!(FillIPatch);
delegate_post!(FillIPatch);
delegate_put!(FillIPatch);
delegate_delete!(FillIPatch);
delegate_fetch!(FillIPatch);

impl<T> super::IPatchRenderable for FillIPatch<T> {
    type IPatch = ();

    // …and the provided impl is good enough
}

impl<
    T: super::GetRenderable
        + super::PostRenderable
        + super::PutRenderable
        + super::DeleteRenderable
        + super::FetchRenderable,
> super::TypeRenderable for FillIPatch<T>
{
}
