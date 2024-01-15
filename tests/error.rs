use coap_message::error::RenderableOnMinimal;
use coap_message::{MessageOption, ReadableMessage};

use coap_handler_implementations::Error;

#[track_caller]
fn assert_just_code(msg: coap_message_utils::inmemory_write::Message, code: u8) {
    assert_eq!(msg.code(), code, "Code was not set correctly");
    assert_eq!(
        msg.options().next().map(|o| o.number()),
        Some(0), // FIXME: That's really testing for a bug in coap_message_utils, should be None
        "Unexpected option set in error response"
    );
    assert_eq!(
        msg.payload(),
        b"",
        "Unexpected payload set in error response"
    );
}

#[track_caller]
fn assert_9290_diag(msg: coap_message_utils::inmemory_write::Message, code: u8, diag: &str) {
    assert_eq!(msg.code(), code, "Code was not set correctly");
    assert_eq!(
        msg.options().next().unwrap().number(),
        coap_numbers::option::CONTENT_FORMAT,
    );
    assert_eq!(
        coap_numbers::content_format::to_str(
            msg.options()
                .next()
                .unwrap()
                .value_uint()
                .expect("Content format is numeric")
        )
        .expect("Content format should be known"),
        "application/concise-problem-details+cbor"
    );
    assert_eq!(
        cbor_diag::parse_bytes(msg.payload())
            .expect("Payload should be CBOR")
            .to_diag(),
        diag
    );
}

#[test]
fn error_simple() {
    let mut code = 0;
    let mut tail = [0; 100];
    let mut msg = coap_message_utils::inmemory_write::Message::new(&mut code, tail.as_mut());

    let err = Error::not_found();
    err.render(&mut msg).unwrap();

    assert_just_code(msg, coap_numbers::code::NOT_FOUND);
}

#[test]
fn error_badoption() {
    let mut code = 0;
    let mut tail = [0; 100];
    let mut msg = coap_message_utils::inmemory_write::Message::new(&mut code, tail.as_mut());

    let err = Error::bad_option(42);
    err.render(&mut msg).unwrap();

    if cfg!(feature = "error_unprocessed_coap_option") {
        // We may need to update this as cbor-diag becomes less verbose
        assert_9290_diag(msg, coap_numbers::code::BAD_OPTION, "{-8:42_0}");
    } else {
        assert_just_code(msg, coap_numbers::code::BAD_OPTION);
    }
}

#[test]
fn error_rbep() {
    let mut code = 0;
    let mut tail = [0; 100];
    let mut msg = coap_message_utils::inmemory_write::Message::new(&mut code, tail.as_mut());

    let err = Error::bad_request_with_rbep(1234);
    err.render(&mut msg).unwrap();

    if cfg!(feature = "error_unprocessed_coap_option") {
        // We may need to update this as cbor-diag becomes less verbose
        assert_9290_diag(msg, coap_numbers::code::BAD_REQUEST, "{-25_0:1234_1}");
    } else {
        assert_just_code(msg, coap_numbers::code::BAD_REQUEST);
    }
}

#[test]
#[cfg(feature = "error_max_age")]
fn error_maxage() {
    let mut code = 0;
    let mut tail = [0; 100];
    let mut msg = coap_message_utils::inmemory_write::Message::new(&mut code, tail.as_mut());

    let err = Error::service_unavailable().with_max_age(0);
    err.render(&mut msg).unwrap();

    assert_eq!(
        msg.code(),
        coap_numbers::code::SERVICE_UNAVAILABLE,
        "Code was not set correctly"
    );
    assert_eq!(
        msg.options().next().map(|o| o.number()),
        Some(coap_numbers::option::MAX_AGE),
        "Max-Age option was not set"
    );
}
