//! The model Test call, `settings-test`: one "ping" to the model the user connected, so the
//! app can say whether the address, the model and the key work.
//!
//! One chat-completions request goes to `<address>/chat/completions` with the key as a bearer
//! token. The answer is one JSON line: `{"result":"works","text":"..."}`, or
//! `{"result":"key refused"}` (401, 403), `{"result":"model not found"}` (404),
//! `{"result":"unreachable"}` (no connection), `{"result":"timeout"}`, or
//! `{"result":"error","status":N}`. A request that cannot be made at all (no address, a bad
//! address or key) answers `{"error":"..."}` like the other commands.
//!
//! Redirects are never followed and no proxy is used, so the key goes only to the address
//! that was checked. The provider's error body is read and dropped, never passed on: it can
//! echo the key. The request line and its arguments are never logged or repeated.

use std::io::Read;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use crate::ipc::error_response;
use crate::settings::{Error, Model, check_base_url, check_key};

/// The model used when neither the request nor the settings name one.
pub const DEFAULT_MODEL: &str = "gpt-6-luna";
/// The whole call, connecting, sending and reading, may take this long.
pub(crate) const DEADLINE: Duration = Duration::from_secs(15);
/// The most of an answer that is read.
const MAX_ANSWER: u64 = 64 * 1024;
/// The most characters of the model's text that are passed on.
const MAX_TEXT_CHARS: usize = 200;
/// "A few tokens", but a reasoning model spends some on thinking first: `gpt-6-luna` with 16
/// sometimes ends with a 400 ("output limit reached"); it needed 7 to 14 for thinking.
const MAX_TOKENS: u32 = 128;
const INSTRUCTION: &str = "Answer with the single word: pong";

/// The values of a `settings-test` request. A missing or empty one comes from the settings.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct Request {
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
}

/// What the call came to.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Works(String),
    KeyRefused,
    ModelNotFound,
    Unreachable,
    Timeout,
    Status(u16),
}

impl Outcome {
    fn to_json(&self) -> String {
        match self {
            Outcome::Works(text) => json!({ "result": "works", "text": text }),
            Outcome::KeyRefused => json!({ "result": "key refused" }),
            Outcome::ModelNotFound => json!({ "result": "model not found" }),
            Outcome::Unreachable => json!({ "result": "unreachable" }),
            Outcome::Timeout => json!({ "result": "timeout" }),
            Outcome::Status(status) => json!({ "result": "error", "status": status }),
        }
        .to_string()
    }
}

/// `value` unless it is missing or empty.
fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.is_empty())
}

/// Run `settings-test` with `argument` (a JSON object, or nothing) over the stored `stored`
/// model, and give the JSON line to answer.
#[must_use]
pub fn run(argument: &str, stored: &Model, deadline: Duration) -> String {
    let request = if argument.is_empty() {
        Request::default()
    } else {
        match serde_json::from_str::<Request>(argument) {
            Ok(request) => request,
            Err(_) => return error_response("invalid settings-test request"),
        }
    };
    let base_url = non_empty(request.base_url).unwrap_or_else(|| stored.base_url.clone());
    let model = non_empty(request.model)
        .or_else(|| non_empty(Some(stored.model.clone())))
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    // The stored key goes only to the stored address: a call aimed at another address must
    // bring its own key.
    let key = non_empty(request.api_key).or_else(|| {
        (base_url == stored.base_url)
            .then(|| non_empty(stored.api_key.clone()))
            .flatten()
    });

    if base_url.is_empty() {
        return error_response("no model address is set");
    }
    if let Err(e) = check_base_url(&base_url) {
        return error_response(e.as_str());
    }
    // The key has to be a header value too: ASCII only.
    if let Some(key) = key.as_deref()
        && (check_key(key).is_err() || !key.is_ascii())
    {
        return error_response(Error::InvalidKey.as_str());
    }
    let Some(url) = completions_url(&base_url) else {
        return error_response(Error::InvalidBaseUrl.as_str());
    };
    ping(&url, &model, key.as_deref(), deadline).to_json()
}

/// `<address>/chat/completions`, without doubling a slash at the end of the address.
fn completions_url(base_url: &str) -> Option<Url> {
    let mut url = Url::parse(base_url).ok()?;
    url.path_segments_mut()
        .ok()?
        .pop_if_empty()
        .extend(["chat", "completions"]);
    Some(url)
}

/// Which parameter limits the length of the answer: new `OpenAI` models take
/// `max_completion_tokens`, older ones and many compatible servers `max_tokens`.
#[derive(Clone, Copy)]
enum LimitField {
    MaxCompletionTokens,
    MaxTokens,
}

fn body(model: &str, limit: LimitField) -> String {
    let field = match limit {
        LimitField::MaxCompletionTokens => "max_completion_tokens",
        LimitField::MaxTokens => "max_tokens",
    };
    let mut body = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": INSTRUCTION },
            { "role": "user", "content": "ping" },
        ],
    });
    body[field] = json!(MAX_TOKENS);
    body.to_string()
}

/// Make the call: with `max_completion_tokens`, and once more with `max_tokens` when the
/// provider answers a 400 that names `max_completion_tokens`. Both share one `deadline`.
fn ping(url: &Url, model: &str, key: Option<&str>, deadline: Duration) -> Outcome {
    let started = Instant::now();
    let first = send(
        url,
        &body(model, LimitField::MaxCompletionTokens),
        key,
        deadline,
    );
    if !first.refused_limit_field {
        return first.outcome;
    }
    let Some(left) = deadline.checked_sub(started.elapsed()) else {
        return Outcome::Timeout;
    };
    send(url, &body(model, LimitField::MaxTokens), key, left).outcome
}

/// What one request came to, and whether it was a 400 that names `max_completion_tokens`.
struct Sent {
    outcome: Outcome,
    refused_limit_field: bool,
}

impl From<Outcome> for Sent {
    fn from(outcome: Outcome) -> Self {
        Self {
            outcome,
            refused_limit_field: false,
        }
    }
}

/// Read at most `MAX_ANSWER` bytes of the answer.
fn read_answer(response: ureq::http::Response<ureq::Body>) -> Result<Vec<u8>, ureq::Error> {
    let mut answer = Vec::new();
    response
        .into_body()
        .into_with_config()
        .limit(MAX_ANSWER)
        .reader()
        .read_to_end(&mut answer)
        // ureq wraps its own error (a timeout) in an `io::Error`; this unwraps it again.
        .map_err(ureq::Error::from)?;
    Ok(answer)
}

fn send(url: &Url, body: &str, key: Option<&str>, deadline: Duration) -> Sent {
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .proxy(None)
        .timeout_global(Some(deadline))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut request = agent
        .post(url.as_str())
        .header("Content-Type", "application/json");
    if let Some(key) = key {
        request = request.header("Authorization", format!("Bearer {key}"));
    }
    let response = match request.send(body) {
        Ok(response) => response,
        Err(e) => return failure(&e).into(),
    };
    let status = response.status().as_u16();
    let answer = match read_answer(response) {
        Ok(answer) => answer,
        // Too long to be a completion, and not worth reading on.
        Err(ureq::Error::BodyExceedsLimit(_)) => Vec::new(),
        Err(e) => return failure(&e).into(),
    };
    match status {
        200..=299 => completion_text(&answer, key)
            .map_or(Outcome::Status(status), Outcome::Works)
            .into(),
        400 => Sent {
            outcome: Outcome::Status(400),
            // The body is only searched here, never passed on.
            refused_limit_field: String::from_utf8_lossy(&answer).contains("max_completion_tokens"),
        },
        401 | 403 => Outcome::KeyRefused.into(),
        404 => Outcome::ModelNotFound.into(),
        _ => Outcome::Status(status).into(),
    }
}

/// The reason for a call that got no answer. The error text is dropped on purpose.
fn failure(error: &ureq::Error) -> Outcome {
    match error {
        ureq::Error::Timeout(_) => Outcome::Timeout,
        _ => Outcome::Unreachable,
    }
}

/// The text of the first choice of a chat completion; empty when it has none (a reasoning
/// model can spend its few tokens on thinking). `None` when the answer is not a completion.
/// The key is scrubbed from the whole text, then the text is cut, so no part of it is passed on.
fn completion_text(answer: &[u8], key: Option<&str>) -> Option<String> {
    let value: Value = serde_json::from_slice(answer).ok()?;
    let choices = value.get("choices")?.as_array()?;
    let text = choices
        .first()
        .and_then(|c| c.pointer("/message/content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let text = match key {
        Some(key) => text.replace(key, "[hidden]"),
        None => text.to_string(),
    };
    Some(text.chars().take(MAX_TEXT_CHARS).collect())
}
