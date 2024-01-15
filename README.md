[![Build Status](https://gitlab.com/chrysn/coap-handler-implementations/badges/master/pipeline.svg)](https://gitlab.com/chrysn/coap-handler-implementations/commits/master)

# coap-handler-implementations

The `coap-handler-implementations` crate provides a few convenience, example or
reference implementations of the [coap-handler] interface.

They range from the generic "4.04
Not Found" responder up to a handler that creates a [write] formatter for GET-only resources,
and even provides [block-wise transfer] that.
The [TypeHandler] enables the easy creation of [serde_cbor] based
resource implementations with GET, PUT and POST support in CBOR format.
The `HandlerBuilder` implements crude static path based routing
that may suffice for some applications, and is also useful to get started quickly.

[coap-handler]: https://crates.io/crates/coap-handler
[write]: https://doc.rust-lang.org/core/fmt/trait.Write.html
[block-wise transfer]: https://tools.ietf.org/html/rfc7959
[serde_cbor]: https://crates.io/crates/serde_cbor

History
-------

This code used to be part of coap-handler, but while the interface needs to stabilize fast (as
incompatible changes there propagate to implementations), the implementations still need to
develop fast (but it doesn't hurt too much if one code part is using the old SimpleRenderable
while another uses a newer NeverFound).

Version 0.1 is what was available in coap-handler (if Cargo allowed cyclic dependencies,
coap-handler would `pub use` this crate); users are encouraged to just use
coap-handler-implementations directly.

Options Hiding
--------------

A common mechanism in handlers is that handlers "consume" options. For example, the
[ForkingHandler] built through [HandlerBuilder::at] "eats" the Uri-Path; likewise, an
Accept-based dispatcher would consume the Accept option.

This allows the handlers themselves to check for any left-over critical options and fail if
they can't handle them -- without the need to mask them out there assuming (without an actual
check) that prior wrappers took care of them.

License: MIT OR Apache-2.0
