//! The chat core for the phone apps, behind a plain C interface.
//!
//! The Kotlin code of both apps calls two functions: one builds the request body of a fixed test
//! conversation (the bytes the provider's prompt cache sees), the other tests a model setting. The
//! interface is C because both sides read it as it is: the iPhone's Kotlin/Native reads the header
//! (`include/construct_chat.h`) and the Android Kotlin calls the shared library by symbol name.
//! Text crosses as UTF-8 C strings; every string the library returns is freed with
//! [`cf_chat_free`].
//!
//! The key crosses as an argument of [`cf_chat_test_model`] and nowhere else: it is never logged,
//! stored or put in a returned text.

use std::ffi::{CStr, CString, c_char};
use std::sync::Arc;

use chat_core::model::Model;
use chat_core::model_test;
use chat_core::tool::{ChatTools, ToolConfig, ToolHandler};
use chat_core::wire::request_body;
use serde_json::json;

pub mod conversation;

/// A handler that is never run: the body only holds the list of tools.
struct Idle(String);

impl ToolHandler for Idle {
    fn name(&self) -> &str {
        &self.0
    }

    fn run(&self, _: &str) -> Result<String, String> {
        Ok(String::new())
    }
}

/// The request body of the fixed conversation over the tool file `tools_file`.
///
/// # Errors
/// With a fixed text when the tool file is not valid.
pub fn fixed_request_body(tools_file: &str) -> Result<String, String> {
    let config = ToolConfig::parse(tools_file).map_err(|e| format!("bad tool file: {e}"))?;
    let handlers: Vec<Arc<dyn ToolHandler>> = config
        .names()
        .into_iter()
        .map(|name| Arc::new(Idle(name.to_string())) as Arc<dyn ToolHandler>)
        .collect();
    let tools =
        ChatTools::configured(&config, &handlers).map_err(|e| format!("bad tool file: {e}"))?;
    request_body(
        conversation::MODEL,
        &conversation::conversation(),
        tools.listing(),
    )
    .map_err(|_| "the body cannot be written".to_string())
}

/// The Test of a model setting: the one JSON line the desktop's Settings page gets, such as
/// `{"result":"works",...}`, `{"result":"key refused",...}` or `{"error":"..."}`.
#[must_use]
pub fn test_model(base_url: &str, model: &str, api_key: &str) -> String {
    let request = json!({ "base_url": base_url, "model": model, "api_key": api_key });
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
        Some(file) => fixed_request_body(&file),
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
        (Some(url), Some(name), Some(key)) => c_string(&test_model(&url, &name, &key)),
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
