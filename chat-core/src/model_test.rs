//! The model Test call, `settings-test`: a list of named checks on the model the user
//! connected, so the app can say whether the address, the model and the key work, and what the
//! model can do.
//!
//! The checks run in order, each with its own deadline, and a check after a failed one is
//! `skipped`. `answers` sends one "ping" and `uses tools` offers one tiny tool and sends its
//! result back; every request goes to `<address>/chat/completions` with the key as a bearer
//! token. The answer is one JSON line: `{"result":"works","text":"...","checks":[...]}`, where
//! `result` is `works` only when every check works, and otherwise the result of the first
//! check that failed. A result is `works`, `key refused` (401, 403), `model not found` (404),
//! `unreachable` (no connection), `timeout`, `error` with the `status`, or one of the
//! results of `uses tools`: `tools refused` (a 400, 404 or 422 to either of its requests),
//! `no tool call`, `bad tool call`, `wrong answer` (the answer has no whole number 5).
//! Each entry of `checks` is `{"name":"answers","result":"works"}`, with the `status` for an
//! `error` and `skipped` for a check that did not run. A request that cannot be made at all
//! (no address, a bad address or key) answers `{"error":"..."}` like the other commands.
//!
//! With `"progress": true` in the request the answer is several lines: `{"checks":[names]}`,
//! then `{"check":{...}}` for each check as soon as it ends (a skipped one too), then the one
//! line above.
//!
//! Redirects are never followed and no proxy is used, so the key goes only to the address
//! that was checked. The provider's error body is read and dropped, never passed on: it can
//! echo the key. The request line and its arguments are never logged or repeated.

use std::io::Read;
use std::time::{Duration, Instant};

use reqwest::Url;
use reqwest::header::{self, HeaderValue};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::model::{
    Error, Model, check_base_url, check_key, refuses_tools_with_reasoning, without_reasoning,
};

/// The JSON line for a refused request; `message` is a fixed text.
fn error_response(message: &str) -> String {
    json!({ "error": message }).to_string()
}

/// The model used when neither the request nor the settings name one.
pub const DEFAULT_MODEL: &str = "gpt-6-luna";
/// Each check, connecting, sending and reading, may take this long.
pub const DEADLINE: Duration = Duration::from_secs(15);
/// The most of an answer that is read.
const MAX_ANSWER: u64 = 64 * 1024;
/// The most characters of the model's text that are passed on.
const MAX_TEXT_CHARS: usize = 200;
/// "A few tokens", but a reasoning model spends some on thinking first: `gpt-6-luna` with 16
/// sometimes ends with a 400 ("output limit reached"); it needed 7 to 14 for thinking.
const MAX_TOKENS: u32 = 128;
/// A tool call needs more room: the model thinks about the tool, then writes the call.
const MAX_TOOL_TOKENS: u32 = 1024;
const INSTRUCTION: &str = "Answer with the single word: pong";
const TOOL_INSTRUCTION: &str = "Use the add tool to add 2 and 3, then tell me the sum.";
const TOOL_NAME: &str = "add";
/// What the tool "returns"; the model has to answer from it.
const TOOL_RESULT: &str = "5";

/// The values of a `settings-test` request. A missing or empty one comes from the settings.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct Request {
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    /// Answer with a line for each check, as it ends, before the last line.
    progress: bool,
}

/// What a check came to.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Works(String),
    KeyRefused,
    ModelNotFound,
    Unreachable,
    Timeout,
    Status(u16),
    /// The provider refused a request that offers tools.
    ToolsRefused,
    /// The model answered with text where it had to call the tool.
    NoToolCall,
    /// The model called a tool that is not there, with arguments that do not fit, or without an id.
    BadToolCall,
    /// The model got the result of its call and did not answer from it.
    WrongAnswer,
}

impl Outcome {
    fn works(&self) -> bool {
        matches!(self, Outcome::Works(_))
    }

    fn to_value(&self) -> Value {
        match self {
            Outcome::Works(text) => json!({ "result": "works", "text": text }),
            Outcome::KeyRefused => json!({ "result": "key refused" }),
            Outcome::ModelNotFound => json!({ "result": "model not found" }),
            Outcome::Unreachable => json!({ "result": "unreachable" }),
            Outcome::Timeout => json!({ "result": "timeout" }),
            Outcome::Status(status) => json!({ "result": "error", "status": status }),
            Outcome::ToolsRefused => json!({ "result": "tools refused" }),
            Outcome::NoToolCall => json!({ "result": "no tool call" }),
            Outcome::BadToolCall => json!({ "result": "bad tool call" }),
            Outcome::WrongAnswer => json!({ "result": "wrong answer" }),
        }
    }
}

/// What a check needs to reach the model.
struct Probe<'a> {
    url: &'a Url,
    model: &'a str,
    key: Option<&'a str>,
}

/// One named check. A check gets its own `deadline`.
pub struct Check {
    name: &'static str,
    run: fn(&Probe, Duration) -> Outcome,
}

impl Check {
    /// The model answers a "ping".
    #[must_use]
    pub fn answers() -> Self {
        Self {
            name: "answers",
            run: ping,
        }
    }

    /// The model calls a tool with the right arguments, then answers from the tool's result.
    #[must_use]
    pub fn uses_tools() -> Self {
        Self {
            name: "uses tools",
            run: use_tool,
        }
    }
}

/// The checks of a Test, in the order they run.
#[must_use]
pub fn checks() -> Vec<Check> {
    vec![Check::answers(), Check::uses_tools()]
}

/// `value` unless it is missing or empty.
fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.is_empty())
}

/// The entry of a check in the `checks` list; `None` is a check that did not run.
fn entry(name: &str, outcome: Option<&Outcome>) -> Value {
    let mut value = outcome.map_or_else(|| json!({ "result": "skipped" }), Outcome::to_value);
    if let Some(fields) = value.as_object_mut() {
        fields.remove("text");
        fields.insert("name".to_string(), json!(name));
    }
    value
}

/// The last line: the result of the first check that failed, or the first check's own (and so
/// its text) when none did, and the entry of every check.
fn summary(checks: &[Check], outcomes: &[Option<Outcome>]) -> String {
    let mut line = outcomes
        .iter()
        .flatten()
        .find(|o| !o.works())
        .or_else(|| outcomes.iter().flatten().next())
        .map_or_else(
            || Outcome::Works(String::new()).to_value(),
            Outcome::to_value,
        );
    line["checks"] = checks
        .iter()
        .zip(outcomes)
        .map(|(check, outcome)| entry(check.name, outcome.as_ref()))
        .collect();
    line.to_string()
}

/// Run `settings-test` with `argument` (a JSON object, or nothing) over the stored `stored`
/// model, with the `checks` of a Test, and hand each line of the answer to `out`: only the
/// last one, or with `progress` in the request a line for each check too. `out` returns
/// `false` when nobody reads the lines any more, and the Test stops.
pub fn run(argument: &str, stored: &Model, deadline: Duration, out: &mut dyn FnMut(&str) -> bool) {
    run_checks(argument, stored, deadline, &checks(), out);
}

/// `run` with the list of `checks` given.
pub fn run_checks(
    argument: &str,
    stored: &Model,
    deadline: Duration,
    checks: &[Check],
    out: &mut dyn FnMut(&str) -> bool,
) {
    let Ok(request) = (if argument.is_empty() {
        Ok(Request::default())
    } else {
        serde_json::from_str::<Request>(argument)
    }) else {
        out(&error_response("invalid settings-test request"));
        return;
    };
    let progress = request.progress;
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
        out(&error_response("no model address is set"));
        return;
    }
    if let Err(e) = check_base_url(&base_url) {
        out(&error_response(e.as_str()));
        return;
    }
    // The key has to be a header value too: ASCII only.
    if let Some(key) = key.as_deref()
        && (check_key(key).is_err() || !key.is_ascii())
    {
        out(&error_response(Error::InvalidKey.as_str()));
        return;
    }
    let Some(url) = completions_url(&base_url) else {
        out(&error_response(Error::InvalidBaseUrl.as_str()));
        return;
    };
    let probe = Probe {
        url: &url,
        model: &model,
        key: key.as_deref(),
    };

    if progress {
        let names: Vec<&str> = checks.iter().map(|c| c.name).collect();
        if !out(&json!({ "checks": names }).to_string()) {
            return;
        }
    }
    let mut outcomes: Vec<Option<Outcome>> = Vec::with_capacity(checks.len());
    let mut failed = false;
    for check in checks {
        let outcome = (!failed).then(|| (check.run)(&probe, deadline));
        failed |= outcome.as_ref().is_some_and(|o| !o.works());
        if progress && !out(&json!({ "check": entry(check.name, outcome.as_ref()) }).to_string()) {
            return;
        }
        outcomes.push(outcome);
    }
    out(&summary(checks, &outcomes));
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

/// A request body with `limit` tokens in the field `field`.
fn with_limit(mut body: Value, field: LimitField, limit: u32) -> String {
    let field = match field {
        LimitField::MaxCompletionTokens => "max_completion_tokens",
        LimitField::MaxTokens => "max_tokens",
    };
    body[field] = json!(limit);
    body.to_string()
}

fn ping_body(model: &str, field: LimitField) -> String {
    let body = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": INSTRUCTION },
            { "role": "user", "content": "ping" },
        ],
    });
    with_limit(body, field, MAX_TOKENS)
}

/// Check `answers`.
fn ping(probe: &Probe, deadline: Duration) -> Outcome {
    let answered = match post(probe, &|field| ping_body(probe.model, field), deadline) {
        Ok(answered) => answered,
        Err(outcome) => return outcome,
    };
    match answered.status {
        200..=299 => completion_text(&answered.body, probe.key)
            .map_or(Outcome::Status(answered.status), Outcome::Works),
        status => refusal(status),
    }
}

/// The outcome of a status that is not a success.
fn refusal(status: u16) -> Outcome {
    match status {
        401 | 403 => Outcome::KeyRefused,
        404 => Outcome::ModelNotFound,
        _ => Outcome::Status(status),
    }
}

/// The tool the model is offered.
fn tools() -> Value {
    json!([{
        "type": "function",
        "function": {
            "name": TOOL_NAME,
            "description": "Add two numbers.",
            "parameters": {
                "type": "object",
                "properties": { "a": { "type": "number" }, "b": { "type": "number" } },
                "required": ["a", "b"],
            },
        },
    }])
}

fn tool_body(model: &str, messages: &[Value], field: LimitField) -> String {
    let body = json!({ "model": model, "messages": messages, "tools": tools() });
    with_limit(body, field, MAX_TOOL_TOKENS)
}

/// What the model made of the offer of the tool.
enum ToolTurn {
    /// A call of the tool, with its id and the message to send back.
    Call {
        id: String,
        message: Value,
    },
    Outcome(Outcome),
}

/// Whether `arguments` (the JSON text of a call) are the numbers 2 and 3, in either order.
fn fits_arguments(arguments: &str) -> bool {
    let Ok(Value::Object(fields)) = serde_json::from_str::<Value>(arguments) else {
        return false;
    };
    let number = |name: &str| fields.get(name).and_then(Value::as_f64);
    let (Some(a), Some(b)) = (number("a"), number("b")) else {
        return false;
    };
    // `3` and `3.0` are the same number.
    let is = |value: f64, want: f64| (value - want).abs() < f64::EPSILON;
    fields.len() == 2 && ((is(a, 2.0) && is(b, 3.0)) || (is(a, 3.0) && is(b, 2.0)))
}

/// Read the first turn: a call of the tool with fitting arguments, or why not.
fn tool_turn(answer: &[u8]) -> ToolTurn {
    let Ok(value) = serde_json::from_slice::<Value>(answer) else {
        return ToolTurn::Outcome(Outcome::Status(200));
    };
    let Some(message) = value
        .pointer("/choices/0/message")
        .filter(|m| m.is_object())
    else {
        return ToolTurn::Outcome(Outcome::Status(200));
    };
    let Some(call) = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .and_then(|calls| calls.first())
    else {
        return ToolTurn::Outcome(Outcome::NoToolCall);
    };
    let function = call.get("function");
    let name = function.and_then(|f| f.get("name")).and_then(Value::as_str);
    let arguments = function
        .and_then(|f| f.get("arguments"))
        .and_then(Value::as_str);
    let Some(arguments) = arguments.filter(|a| name == Some(TOOL_NAME) && fits_arguments(a)) else {
        return ToolTurn::Outcome(Outcome::BadToolCall);
    };
    // The result is sent back under the call's id, so a call without one cannot be answered.
    let Some(id) = call
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
    else {
        return ToolTurn::Outcome(Outcome::BadToolCall);
    };
    // Send the call back as it was made, but only the parts the provider has to know.
    let message = json!({
        "role": "assistant",
        "content": null,
        "tool_calls": [{
            "id": id,
            "type": "function",
            "function": { "name": TOOL_NAME, "arguments": arguments },
        }],
    });
    ToolTurn::Call { id, message }
}

/// Check `uses tools`: offer the tool, take the call, send the result, read the answer. Both
/// requests share the one `deadline`.
fn use_tool(probe: &Probe, deadline: Duration) -> Outcome {
    let started = Instant::now();
    let mut messages = vec![json!({ "role": "user", "content": TOOL_INSTRUCTION })];
    let first = match post(
        probe,
        &|field| tool_body(probe.model, &messages, field),
        deadline,
    ) {
        Ok(answered) => answered,
        Err(outcome) => return outcome,
    };
    let first = match accepted(first) {
        Ok(answered) => answered,
        Err(outcome) => return outcome,
    };
    let (id, call) = match tool_turn(&first.body) {
        ToolTurn::Call { id, message } => (id, message),
        ToolTurn::Outcome(outcome) => return outcome,
    };
    messages.push(call);
    messages.push(json!({ "role": "tool", "tool_call_id": id, "content": TOOL_RESULT }));

    let Some(left) = deadline.checked_sub(started.elapsed()) else {
        return Outcome::Timeout;
    };
    let second = match post(
        probe,
        &|field| tool_body(probe.model, &messages, field),
        left,
    ) {
        Ok(answered) => answered,
        Err(outcome) => return outcome,
    };
    let second = match accepted(second) {
        Ok(answered) => answered,
        Err(outcome) => return outcome,
    };
    let Some(content) = completion_content(&second.body) else {
        return Outcome::Status(second.status);
    };
    // The whole number, on the whole text: "15" is not 5, and a 5 after a long thought counts.
    if content
        .split(|c: char| !c.is_ascii_digit())
        .any(|number| number == TOOL_RESULT)
    {
        // Nothing reads the text of this check: only the first check's text is passed on.
        Outcome::Works(String::new())
    } else {
        Outcome::WrongAnswer
    }
}

/// The answer to a request that offers tools, when it is a success. Any 400, 404 or 422 is
/// `tools refused`, after the tool message as well as before it.
fn accepted(answered: Answered) -> Result<Answered, Outcome> {
    match answered.status {
        200..=299 => Ok(answered),
        400 | 404 | 422 => Err(Outcome::ToolsRefused),
        status => Err(refusal(status)),
    }
}

/// What the provider answered: the status and the body that was read.
struct Answered {
    status: u16,
    body: Vec<u8>,
}

/// Make the call: with `max_completion_tokens`, and once more with `max_tokens` when the
/// provider answers a 400 that names `max_completion_tokens`. A 400 that refuses function tools
/// next to a reasoning effort is answered with the same request and `reasoning_effort: "none"`.
/// All of them share one `deadline`. Fails with what a request that got no answer came to.
fn post(
    probe: &Probe,
    body: &dyn Fn(LimitField) -> String,
    deadline: Duration,
) -> Result<Answered, Outcome> {
    let started = Instant::now();
    // The request for a limit field, with `reasoning_effort: "none"` once the provider asked for it.
    let mut reasoning_off = false;
    let request = |field, reasoning_off: bool| {
        let request = body(field);
        if reasoning_off {
            without_reasoning(request.as_bytes())
                .and_then(|plain| String::from_utf8(plain).ok())
                .unwrap_or(request)
        } else {
            request
        }
    };
    let mut first = send(
        probe,
        &request(LimitField::MaxCompletionTokens, reasoning_off),
        deadline,
    )?;
    // The body is only searched here, never passed on.
    if refuses_tools_with_reasoning(first.status, &first.body) {
        reasoning_off = true;
        let Some(left) = deadline.checked_sub(started.elapsed()) else {
            return Err(Outcome::Timeout);
        };
        first = send(
            probe,
            &request(LimitField::MaxCompletionTokens, reasoning_off),
            left,
        )?;
    }
    let refused_limit_field = first.status == 400
        && String::from_utf8_lossy(&first.body).contains("max_completion_tokens");
    if !refused_limit_field {
        return Ok(first);
    }
    let Some(left) = deadline.checked_sub(started.elapsed()) else {
        return Err(Outcome::Timeout);
    };
    send(probe, &request(LimitField::MaxTokens, reasoning_off), left)
}

/// Why a request got no answer: the reason a [`failure`] comes to.
enum Unanswered {
    Timeout,
    Other,
}

/// The reason of a failed read of the body. A timeout of the whole request can come wrapped in an
/// `io::Error`, which this unwraps again.
fn unread(error: &std::io::Error) -> Unanswered {
    let timed_out = error.kind() == std::io::ErrorKind::TimedOut
        || error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<reqwest::Error>())
            .is_some_and(reqwest::Error::is_timeout);
    if timed_out {
        Unanswered::Timeout
    } else {
        Unanswered::Other
    }
}

/// Read at most `MAX_ANSWER` bytes of the answer; `None` for one that is longer.
fn read_answer(response: reqwest::blocking::Response) -> Result<Option<Vec<u8>>, Unanswered> {
    let mut answer = Vec::new();
    response
        .take(MAX_ANSWER + 1)
        .read_to_end(&mut answer)
        .map_err(|e| unread(&e))?;
    Ok((answer.len() as u64 <= MAX_ANSWER).then_some(answer))
}

/// One request, and the answer to it. The whole request has `deadline` as the limit, and the
/// answer is read to `deadline` from the thread that waits for it (see [`send`]).
fn send(probe: &Probe, body: &str, deadline: Duration) -> Result<Answered, Outcome> {
    // The client's own timeout covers only the wait for the head of the answer: every read of the
    // body gets the whole time again, so a server that sends a byte now and then could hold a
    // reader for a long while. The one limit on the whole request is the wait here; the thread that
    // does the request is left to end by itself, within the client's limits and the cap of the
    // answer.
    let (url, key, body) = (
        probe.url.clone(),
        probe.key.map(str::to_owned),
        body.to_owned(),
    );
    let (done, answer) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(exchange(&url, key.as_deref(), &body, deadline));
    });
    match answer.recv_timeout(deadline) {
        Ok(answered) => answered,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(Outcome::Timeout),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(Outcome::Unreachable),
    }
}

fn exchange(
    url: &Url,
    key: Option<&str>,
    body: &str,
    deadline: Duration,
) -> Result<Answered, Outcome> {
    // Redirects are never followed and no proxy is used, so the key goes only to the address that
    // was checked.
    let client = reqwest::blocking::Client::builder()
        .timeout(deadline)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|_| Outcome::Unreachable)?;
    let mut request = client
        .post(url.clone())
        .header(header::CONTENT_TYPE, "application/json")
        .body(body.to_string());
    if let Some(key) = key {
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {key}")).map_err(|_| Outcome::Unreachable)?;
        authorization.set_sensitive(true);
        request = request.header(header::AUTHORIZATION, authorization);
    }
    let response = request.send().map_err(|e| failure(&e))?;
    let status = response.status().as_u16();
    let body = match read_answer(response) {
        Ok(Some(answer)) => answer,
        // Too long to be a completion, and not worth reading on.
        Ok(None) => Vec::new(),
        Err(Unanswered::Timeout) => return Err(Outcome::Timeout),
        Err(Unanswered::Other) => return Err(Outcome::Unreachable),
    };
    Ok(Answered { status, body })
}

/// The reason for a call that got no answer. The error text is dropped on purpose.
fn failure(error: &reqwest::Error) -> Outcome {
    // The one deadline covers the connect too: a connect that ran out of it is a timeout.
    if error.is_timeout() {
        Outcome::Timeout
    } else {
        Outcome::Unreachable
    }
}

/// The text of the first choice of a chat completion; empty when it has none (a reasoning
/// model can spend its few tokens on thinking). `None` when the answer is not a completion.
/// The key is scrubbed from the whole text, then the text is cut, so no part of it is passed on.
fn completion_text(answer: &[u8], key: Option<&str>) -> Option<String> {
    let text = completion_content(answer)?;
    let text = match key {
        Some(key) => text.replace(key, "[hidden]"),
        None => text,
    };
    Some(text.chars().take(MAX_TEXT_CHARS).collect())
}

/// The whole text of the first choice, as the provider wrote it; see `completion_text`.
fn completion_content(answer: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(answer).ok()?;
    let choices = value.get("choices")?.as_array()?;
    let text = choices
        .first()
        .and_then(|c| c.pointer("/message/content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(text.to_string())
}
