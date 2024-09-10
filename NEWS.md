# Changs in 0.5.2

* Support minicbor 0.24 for TypeRenderable.
* Documentation updates.

# Changs in 0.5.1

* Provide Empty type for TypeRenderable that represents empty payloads in minicbor.
* Populate a few details from minicbor into response errors (shown only if enabled in coap-message-utils).
* Update Rust version to 1.79.
* Documentation updates.
* Deny unsafe code.
* Clean up code around deprecated `_nontrivial_options_processing` feature.

# Changes in 0.5.0

*This is a breaking change relative to alpha.2*

* Move parts over to the `coap-message-utils` crate:

  - the `Error` type
  - all option iteration related triaits and types: `OptionsExt`, `TryFromOption` and all block types

  That is a new public dependency.

* Move option processing over to `coap-message-utils`.

# Changes in 0.5.0-alpha.2

*This release contains no breaking changes relative to alpha.1.*

* Added `Error::with_max_age()` and the `error_max_age` feature.

* Added `Error::service_unavailable()`.

* Documentation enhancements.

# Changes in 0.5.0-alpha.1

* Update coap-message to 0.3.0-alpha.1
  (the "fallible write operations" release).

  The `codeconvert` function is removed,
  because the error from conversion can now be `?`ed.

* Provide a public `Error` struct
  that is used in all fallible operations,
  and also recommended for use by applications as an error type.

* Remove all deprecated items:
  Instead of `Block2RequestData.before()`, use `.start()`.

* Add Block1Data struct.

* Add `take_into()` on options
  (using the new `TryFromOption` trait).
