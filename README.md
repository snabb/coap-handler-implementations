# coap-handler-implementations ![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue) [![coap-handler-implementations on crates.io](https://img.shields.io/crates/v/coap-handler-implementations)](https://crates.io/crates/coap-handler-implementations) [![coap-handler-implementations on docs.rs](https://docs.rs/coap-handler-implementations/badge.svg)](https://docs.rs/coap-handler-implementations) [![Source Code Repository](https://img.shields.io/badge/Code-On%20Codeberg-blue?logo=Codeberg)](https://codeberg.org/chrysn/coap-tools) ![Rust Version: 1.85.0](https://img.shields.io/badge/rustc-1.85.0-orange.svg)

The `coap-handler-implementations` crate provides a few convenience, example or
reference implementations of the [coap-handler][__link0] interface.

They range from the generic “4.04
Not Found” responder up to a handler that creates a [write][__link1] formatter for GET-only resources,
and even provides [block-wise transfer][__link2] that.
The [TypeHandler][__link3] enables the easy creation of [serde_cbor][__link4] based
resource implementations with GET, PUT and POST support in CBOR format.
The `HandlerBuilder` implements crude static path based routing
that may suffice for some applications, and is also useful to get started quickly.

### History

This code used to be part of coap-handler, but while the interface needs to stabilize fast (as
incompatible changes there propagate to implementations), the implementations still need to
develop fast (but it doesn’t hurt too much if one code part is using the old SimpleRenderable
while another uses a newer NeverFound).

Version 0.1 is what was available in coap-handler (if Cargo allowed cyclic dependencies,
coap-handler would `pub use` this crate); users are encouraged to just use
coap-handler-implementations directly.

### Options Hiding

A common mechanism in handlers is that handlers “consume” options. For example, the
[ForkingHandler][__link5] built through [HandlerBuilder::at][__link6] “eats” the Uri-Path; likewise, an
Accept-based dispatcher would consume the Accept option.

This allows the handlers themselves to check for any left-over critical options and fail if
they can’t handle them – without the need to mask them out there assuming (without an actual
check) that prior wrappers took care of them.


 [__cargo_doc2readme_dependencies_info]: ggGkYW0BYXSEG_W_Gn_kaocAGwCcVPfenh7eGy6gYLEwyIe4G6-xw_FwcbpjYXKEG8lqJecE4DHCG0ewgIvUdAZjG8nCMtXH4CzTG1uW4q9Occk6YWSBg3gcY29hcC1oYW5kbGVyLWltcGxlbWVudGF0aW9uc2UwLjUuNHgcY29hcF9oYW5kbGVyX2ltcGxlbWVudGF0aW9ucw
 [__link0]: https://crates.io/crates/coap-handler
 [__link1]: https://doc.rust-lang.org/core/fmt/trait.Write.html
 [__link2]: https://tools.ietf.org/html/rfc7959
 [__link3]: https://docs.rs/coap-handler-implementations/0.5.4/coap_handler_implementations/?search=TypeHandler
 [__link4]: https://crates.io/crates/serde_cbor
 [__link5]: https://docs.rs/coap-handler-implementations/0.5.4/coap_handler_implementations/?search=ForkingHandler
 [__link6]: https://docs.rs/coap-handler-implementations/0.5.4/coap_handler_implementations/?search=HandlerBuilder::at
