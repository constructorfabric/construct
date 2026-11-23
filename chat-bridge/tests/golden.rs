//! The bridge builds the stored bodies of the chat core's golden tests, byte for byte, through the
//! same C functions the apps call.
#![allow(clippy::unwrap_used)]

use std::ffi::{CStr, CString};
use std::ptr;

use construct_chat_bridge::{cf_chat_fixed_request_body, cf_chat_free};

fn through_c(tools: &str) -> (i32, String) {
    let tools = CString::new(tools).unwrap();
    let mut out = ptr::null_mut();
    // SAFETY: `tools` is a C string and `out` is writable; the result is freed once.
    unsafe {
        let code = cf_chat_fixed_request_body(tools.as_ptr(), &raw mut out);
        let text = CStr::from_ptr(out).to_str().unwrap().to_owned();
        cf_chat_free(out);
        (code, text)
    }
}

#[test]
fn the_phone_list_gives_the_stored_body() {
    let (code, body) = through_c(include_str!("../../tools/phone.json"));
    assert_eq!(code, 0);
    let stored = include_str!("../../chat-core/tests/golden/phone.json");
    assert_eq!(body, stored.strip_suffix('\n').unwrap_or(stored));
}

#[test]
fn a_bad_tool_file_gives_a_reason() {
    let (code, text) = through_c("not json");
    assert_eq!(code, 1);
    assert!(text.starts_with("bad tool file"), "{text}");
}
