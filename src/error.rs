//! Common error types

/// A build-time-flexible renderable error type
///
/// This is used wherever this crate produces errors, and also recommended for outside handlers --
/// the idea being that the more code parts share a type, the more compact code can be emitted.
///
/// Depending on what gets configured, it may just be a single u8 error code, or it may contain
/// more details such as the number of the option that could not be processed, or any other
/// Standard Problem Details (RFC9290).
#[derive(Debug)]
pub struct Error {
    code: u8,
    #[cfg(feature = "error_unprocessed_coap_option")]
    unprocessed_option: Option<core::num::NonZeroU16>,
    #[cfg(feature = "error_request_body_error_position")]
    request_body_error_position: Option<u32>,
    #[cfg(feature = "error_max_age")]
    max_age: Option<u32>,
}

impl Error {
    const MAX_ENCODED_LEN: usize = {
        let mut count = 0;
        count += 1; // up to 24 items
        if cfg!(feature = "error_unprocessed_coap_option") {
            // 1 item, 1+0 key, value up to 16bit
            count += 1 + 3;
        }
        if cfg!(feature = "error_request_body_error_position") {
            // 1 item, 1+1 key, value up to 64bit in theory
            count += 2 + 5;
        }
        count
    };

    /// Create an error response for an unprocessed option
    ///
    /// The response is rendered with a single unprocessed-coap-option problem detail if that
    /// feature is enabled.
    ///
    /// If the unprocessed option has a special error code (e.g. Accept => 4.06 Not Acceptable or
    /// Content Format => 4.15 Unsupported Content-Format), that code is emitted instead of the
    /// default 4.02 Bad Option, and the unprocessed-coap-option problem details is not emitted.
    ///
    /// Note that the CoAP option number is given as a u16, as is common around the CoAP crates
    /// (even though 0 is a reseved value). It is an error to pass in the value 0; the
    /// implementation may treat this as a reason for a panic, or silently ignore the error and not
    /// render the problem detail.
    pub fn bad_option(unprocessed_option: u16) -> Self {
        let special_code = match unprocessed_option {
            coap_numbers::option::ACCEPT => Some(coap_numbers::code::NOT_ACCEPTABLE),
            coap_numbers::option::PROXY_URI | coap_numbers::option::PROXY_SCHEME => {
                Some(coap_numbers::code::PROXYING_NOT_SUPPORTED)
            }
            coap_numbers::option::CONTENT_FORMAT => {
                Some(coap_numbers::code::UNSUPPORTED_CONTENT_FORMAT)
            }
            _ => None,
        };

        #[allow(unused)]
        let unprocessed_option = if special_code.is_some() {
            None
        } else {
            core::num::NonZeroU16::try_from(unprocessed_option).ok()
        };

        let code = special_code.unwrap_or(coap_numbers::code::BAD_OPTION);

        Self {
            code,
            #[cfg(feature = "error_unprocessed_coap_option")]
            unprocessed_option,
            ..Self::otherwise_empty()
        }
    }

    /// Create a 4.00 Bad Request error with a Request Body Error Position indicating at which
    /// position in the request's body the error occurred
    ///
    /// If the crate is compiled without the `error_request_body_error_position` feature, the
    /// position information will be ignored. The value may also be ignored if it exceeds an
    /// internal limit of how large values can be expressed.
    pub fn bad_request_with_rbep(#[allow(unused)] byte: usize) -> Self {
        Self {
            code: coap_numbers::code::BAD_REQUEST,
            #[cfg(feature = "error_request_body_error_position")]
            request_body_error_position: byte.try_into().ok(),
            ..Self::otherwise_empty()
        }
    }

    /// Is there any reason to even start rendering CBOR?
    ///
    /// This provides a precise number of items.
    #[inline(always)]
    fn problem_details_count(&self) -> u8 {
        #[allow(unused_mut)]
        let mut count = 0;

        #[cfg(feature = "error_unprocessed_coap_option")]
        if self.unprocessed_option.is_some() {
            count += 1;
        }
        #[cfg(feature = "error_request_body_error_position")]
        if self.request_body_error_position.is_some() {
            count += 1;
        }

        count
    }

    /// Create an otherwise empty 4.00 Bad Request error
    pub fn bad_request() -> Self {
        Self {
            code: coap_numbers::code::BAD_REQUEST,
            ..Self::otherwise_empty()
        }
    }

    /// Create an otherwise empty 4.04 Not Found error
    pub fn not_found() -> Self {
        Self {
            code: coap_numbers::code::NOT_FOUND,
            ..Self::otherwise_empty()
        }
    }

    /// Create an otherwise empty 4.05 Method Not Allowed error
    pub fn method_not_allowed() -> Self {
        Self {
            code: coap_numbers::code::METHOD_NOT_ALLOWED,
            ..Self::otherwise_empty()
        }
    }

    /// Create an otherwise empty 5.03 Service Unavailable error
    pub fn service_unavailable() -> Self {
        Self {
            code: coap_numbers::code::SERVICE_UNAVAILABLE,
            ..Self::otherwise_empty()
        }
    }

    /// Set a Max-Age
    ///
    /// Unlike the constructors that set error details, this modifier is only available when the
    /// data is actually stored, because not emitting it is not just elision of possibly helful
    /// details, but may change the networks' behavior (for example, because a long Max-Age is not
    /// sent and the client keeps retrying every minute).
    #[cfg(feature = "error_max_age")]
    pub fn with_max_age(self, max_age: u32) -> Self {
        Self {
            max_age: Some(max_age),
            ..self
        }
    }

    /// A default-ish constructor that leaves the code in an invalid state -- useful for other
    /// constructors so they only have to cfg() out the values they need, and not every single
    /// line.
    #[inline]
    fn otherwise_empty() -> Self {
        Self {
            code: 0,
            #[cfg(feature = "error_unprocessed_coap_option")]
            unprocessed_option: None,
            #[cfg(feature = "error_request_body_error_position")]
            request_body_error_position: None,
            #[cfg(feature = "error_max_age")]
            max_age: None,
        }
    }
}

impl coap_message::error::RenderableOnMinimal for Error {
    type Error<IE: coap_message::error::RenderableOnMinimal + core::fmt::Debug> = IE;

    fn render<M: coap_message::MinimalWritableMessage>(
        self,
        message: &mut M,
    ) -> Result<(), Self::Error<M::UnionError>> {
        use coap_message::{Code, OptionNumber};

        message.set_code(M::Code::new(self.code)?);

        // In a minimal setup, this is unconditionally 0, and the rest of the problem details stuff
        // should not be emitted in optimized code. If max_age is off too, the optimized function
        // just returns here already.
        let mut pd_count = self.problem_details_count();

        if pd_count > 0 {
            // That's quite liktely to be Some, as that registry is stable
            const PROBLEM_DETAILS: Option<u16> =
                coap_numbers::content_format::from_str("application/concise-problem-details+cbor");
            // May err on stacks that can't do Content-Format (but that's rare).
            let cfopt = M::OptionNumber::new(coap_numbers::option::CONTENT_FORMAT);

            if let Some((pd, cfopt)) = PROBLEM_DETAILS.zip(cfopt.ok()) {
                // If this goes wrong, we rather send the empty response (lest the CBOR be
                // interpreted as plain text diagnostic payload) -- better to send the unannotated
                // code than send even less information by falling back to an internal server error
                // message.
                if message.add_option_uint(cfopt, pd).is_err() {
                    pd_count = 0;
                }
            }
        };

        #[cfg(feature = "error_max_age")]
        if let Some(max_age) = self.max_age {
            // Failure to set this is critical in the sense that we better report it as an internal
            // server error. If problem details are involved, the server should remove that in its
            // error path.
            message.add_option_uint(
                M::OptionNumber::new(coap_numbers::option::MAX_AGE)?,
                max_age,
            )?;
        }

        if pd_count > 0 {
            // FIXME: We could avoid copying if we rendered to anything more than a minimal writable
            // message. (This either needs an ecosystem-wide fix by allowing errors also for more
            // complex message types, or something kind of a downcast ladder).
            //
            // This is not giving us any benefits on the side of not writing partial payloads: If
            // we leave the handler with an error, the server needs to rewind the message anyway.
            let mut buf = [0u8; Self::MAX_ENCODED_LEN];
            if let Ok(written) = (|| {
                // A manual try!{}
                let mut cursor = minicbor::encode::write::Cursor::new(buf.as_mut());
                let mut encoder = minicbor::Encoder::new(&mut cursor);
                #[allow(unused_mut)]
                let mut encoder = encoder.map(pd_count.into())?;

                #[cfg(feature = "error_unprocessed_coap_option")]
                if let Some(unprocessed_option) = self.unprocessed_option {
                    encoder = encoder.i8(-8)?;
                    encoder = encoder.u16(unprocessed_option.into())?;
                }
                #[cfg(feature = "error_request_body_error_position")]
                if let Some(position) = self.request_body_error_position {
                    encoder = encoder.i8(-25)?;
                    encoder = encoder.u32(position)?;
                }
                let _ = encoder;
                let written = cursor.position();
                Ok::<_, minicbor::encode::Error<_>>(written)
            })() {
                message.set_payload(&buf[..written])?;
            }
        }

        Ok(())
    }
}
