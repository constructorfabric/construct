//! The chat core for the phone apps, behind a plain C interface.
//!
//! Both apps use this interface to answer a conversation, test a model setting and check the bytes
//! of a fixed test conversation. The iPhone's Kotlin/Native reads the header
//! (`include/construct_chat.h`) and the Android Kotlin calls the shared library by symbol name.
//! Text crosses as UTF-8 C strings; every string the library returns is freed with
//! [`cf_chat_free`].
//!
//! The one other way in is the JNI function of the Android certificate check (`android_tls`): it
//! hands the JVM to the TLS stack and is not part of the C header.
//!
//! Model keys cross only in call arguments and are never logged, stored or returned.

use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};

use chat_core::fixture::request_body_over;
use chat_core::model::Model;
use chat_core::model_test;
use serde_json::json;

mod answer;

/// The request body of the fixed conversation over the tool file `tools_file`.
///
/// # Errors
/// With a fixed text when the tool file is not valid.
pub fn fixed_request_body(tools_file: &str) -> Result<String, String> {
    request_body_over(tools_file)
}

/// The Test of a model setting: the one JSON line the desktop's Settings page gets, such as
/// `{"result":"works",...}`, `{"result":"key refused",...}` or `{"error":"..."}`.
#[must_use]
pub fn test_model(base_url: &str, model: &str, api_key: &str) -> String {
    last_line(&json!({ "base_url": base_url, "model": model, "api_key": api_key }))
}

/// The last line the Test writes for `request`: the answer is always the last one, and with
/// `"progress": true` in the request a line for every check comes before it.
fn last_line(request: &serde_json::Value) -> String {
    let mut last = String::new();
    model_test::run(
        &request.to_string(),
        &Model::default(),
        model_test::DEADLINE,
        &mut |line| {
            line.clone_into(&mut last);
            true
        },
    );
    last
}

/// The text of the answer when the core panics: a panic must not unwind across the C border.
const INTERNAL_ERROR: &str = "internal error";

/// Runs `work`; a panic in it gives `fallback()` instead of unwinding into the host app.
fn guarded<T>(work: impl FnOnce() -> T, fallback: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(work)).unwrap_or_else(|_| fallback())
}

/// The system's certificate check on Android needs the JVM: Kotlin calls this once, with the app's
/// context, before the first request to an `https` address.
#[cfg(target_os = "android")]
mod android_tls {
    use jni::EnvUnowned;
    use jni::errors::{Error, ThrowRuntimeExAndDefault};
    use jni::objects::JObject;

    /// `AndroidTls.nativeInit(context)` of the Kotlin module.
    #[unsafe(no_mangle)]
    #[allow(non_snake_case)]
    pub extern "system" fn Java_cf_construct_mobile_core_chat_AndroidTls_nativeInit<'caller>(
        mut env: EnvUnowned<'caller>,
        _this: JObject<'caller>,
        context: JObject<'caller>,
    ) {
        env.with_env(|env| -> Result<(), Error> {
            rustls_platform_verifier::android::init_with_env(env, context)
        })
        .resolve::<ThrowRuntimeExAndDefault>();
    }
}

/// A C string from `text`; a text with a NUL byte (none of ours has one) comes out empty.
fn c_string(text: &str) -> *mut c_char {
    CString::new(text).unwrap_or_default().into_raw()
}

/// The text of a C string argument, or `None` when it is null or not UTF-8.
///
/// # Safety
/// `text` is null or points to a NUL-terminated string that stays valid for the call.
unsafe fn argument(text: *const c_char) -> Option<String> {
    if text.is_null() {
        return None;
    }
    // SAFETY: the caller promises a valid NUL-terminated string.
    unsafe { CStr::from_ptr(text) }
        .to_str()
        .ok()
        .map(str::to_owned)
}

/// Build the request body of the fixed conversation over the tool file `tools_file`.
///
/// Returns 0 and sets `*out` to the body, or 1 and sets `*out` to the plain reason. Free `*out`
/// with [`cf_chat_free`].
///
/// # Safety
/// `tools_file` is a NUL-terminated string and `out` points to a writable `char *`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cf_chat_fixed_request_body(
    tools_file: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    if out.is_null() {
        return 1;
    }
    // SAFETY: the caller promises a valid string.
    let result = match unsafe { argument(tools_file) } {
        Some(file) => guarded(
            || fixed_request_body(&file),
            || Err(INTERNAL_ERROR.to_string()),
        ),
        None => Err("the tool file is not text".to_string()),
    };
    let (code, text) = match result {
        Ok(body) => (0, body),
        Err(reason) => (1, reason),
    };
    // SAFETY: the caller promises that `out` is writable.
    unsafe { *out = c_string(&text) };
    code
}

/// Test a model setting: returns the JSON line of [`test_model`]. A null or non-text argument
/// gives `{"error":"invalid argument"}`. Free the result with [`cf_chat_free`].
///
/// # Safety
/// Each argument is a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cf_chat_test_model(
    base_url: *const c_char,
    model: *const c_char,
    api_key: *const c_char,
) -> *mut c_char {
    // SAFETY: the caller promises valid strings.
    let (url, name, key) = unsafe { (argument(base_url), argument(model), argument(api_key)) };
    match (url, name, key) {
        (Some(url), Some(name), Some(key)) => c_string(&guarded(
            || test_model(&url, &name, &key),
            || json!({ "error": INTERNAL_ERROR }).to_string(),
        )),
        _ => c_string(r#"{"error":"invalid argument"}"#),
    }
}

/// Free a string the library returned. Null is allowed.
///
/// # Safety
/// `text` came from this library and is freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cf_chat_free(text: *mut c_char) {
    if !text.is_null() {
        // SAFETY: the caller promises that `text` came from `CString::into_raw` here.
        drop(unsafe { CString::from_raw(text) });
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn the_answer_is_the_last_line_even_when_every_check_writes_one() {
        // A port where nothing listens: every request is refused at once.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let url = format!("http://127.0.0.1:{port}");
        for progress in [false, true] {
            let request = json!({
                "base_url": url,
                "model": "m",
                "api_key": "sk-fake",
                "progress": progress,
            });
            let line = last_line(&request);
            let answer: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(answer["result"], "unreachable", "{line}");
            assert!(!line.contains("sk-fake"));
        }
    }

    #[test]
    fn a_panic_in_the_core_is_an_answer_and_not_an_unwind() {
        let answer = guarded(
            || -> String { panic!("a failure inside the core") },
            || json!({ "error": INTERNAL_ERROR }).to_string(),
        );
        assert_eq!(answer, r#"{"error":"internal error"}"#);
    }

    #[test]
    fn a_null_argument_is_an_invalid_argument_and_not_a_crash() {
        let text = std::ffi::CString::new("x").unwrap();
        // SAFETY: the pointers are valid C strings or null, and the result is freed once.
        let answer = unsafe {
            let out = cf_chat_test_model(std::ptr::null(), text.as_ptr(), text.as_ptr());
            let answer = CStr::from_ptr(out).to_str().unwrap().to_owned();
            cf_chat_free(out);
            answer
        };
        assert_eq!(answer, r#"{"error":"invalid argument"}"#);
    }

    #[test]
    fn a_text_that_is_not_utf8_is_refused() {
        let bad = std::ffi::CString::new(vec![0xff, 0xfe]).unwrap();
        // SAFETY: `bad` is a valid C string and the result is freed once.
        let answer = unsafe {
            let out = cf_chat_test_model(bad.as_ptr(), bad.as_ptr(), bad.as_ptr());
            let answer = CStr::from_ptr(out).to_str().unwrap().to_owned();
            cf_chat_free(out);
            answer
        };
        assert_eq!(answer, r#"{"error":"invalid argument"}"#);
    }
}
