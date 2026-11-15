//! The chat loop behind `chat-send`, `chat-stop`, `chat-list`, `chat-get` and `chat-delete`.
//!
//! A conversation is a session of the source `chat` in the record store; `chat-send` streams the
//! provider's answer to the client as JSON lines. The history never travels over the socket,
//! the loop calls nothing in Construct and never calls `save_progress`.
//!
//! The request body is written from typed structs in a fixed field order, never through a
//! `serde_json::Value` (it sorts keys), with tool-call arguments as the stored text, so a request
//! starts with the bytes of the one before it and the provider's prompt cache can use that.
//!
//! No log line, event or error holds the key or a message text: failures are named by reason
//! only, and the provider's error body is read for one question and dropped.

use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use connectors::{Record, Role, Turn};
use futures_util::StreamExt;
use http::StatusCode;
use oagw_sdk::{ServerEvent, ServerEventsResponse, ServerEventsStream, StreamingError};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::runtime::Handle;
use tokio::time::{Instant, timeout, timeout_at};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::ipc::error_response;
use crate::model::{ANSWER_LIMIT, Target, read_capped};
use crate::records::FileSink;
use crate::settings::Settings;
use crate::sync::lock;

mod wire;

use wire::{Chunk, WireCap, into_http, request_body};

/// The source the conversations belong to.
pub const SOURCE: &str = "chat";
/// Longest `chat-send` request line, in bytes: a pasted message can be long.
pub const MAX_REQUEST: usize = 1024 * 1024;
/// Most bytes of the answer text. The answer ends as `cut` when the next piece would pass it.
const TEXT_LIMIT: usize = ANSWER_LIMIT;
/// Most bytes read from the provider for one answer. A chunk of a few characters is 200 bytes
/// of JSON, so this is a large multiple of [`TEXT_LIMIT`]; it only guards against a stream
/// that never ends, and what the parser buffers of one unfinished event.
const WIRE_LIMIT: usize = 64 * TEXT_LIMIT;
/// Most bytes of an error body that are read, to tell a conversation that is too long.
const ERROR_BODY_LIMIT: usize = 64 * 1024;
const ERROR_BODY_DEADLINE: Duration = Duration::from_secs(5);
/// A conversation is listed under its first message cut to this many characters.
const TITLE_CHARS: usize = 60;
/// A client that does not read for this long is gone.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// The one system instruction. It never changes, because the provider's prompt cache needs the
/// same start for every request.
pub const INSTRUCTION: &str = "You are the chat of a person's Construct, the assistant that lives in their Construct Browser. \
You cannot save anything yourself. Whatever the user tells you is remembered later by the system, on its own, \
unless the `recall_facts` tool returns the error that memory is off. So when the user tells you something worth \
keeping, say that you will remember it, and never say that you have saved it. If `recall_facts` returns the error \
that memory is off, say that nothing is remembered. Answer in the language of the user.";

/// The limits on the wait for the provider.
#[derive(Clone, Copy, Debug)]
pub struct Deadlines {
    pub connect: Duration,
    /// From the request to the first chunk of the answer.
    pub first_word: Duration,
    /// Silence between two chunks, once the first has come.
    pub silence: Duration,
}

impl Default for Deadlines {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(15),
            first_word: Duration::from_secs(60),
            silence: Duration::from_secs(60),
        }
    }
}

/// Why an answer failed. The reasons of the Test call, and `too long` and `cut`.
#[derive(Debug, PartialEq, Eq)]
enum Failure {
    KeyRefused,
    ModelNotFound,
    Unreachable,
    Timeout,
    Status(u16),
    TooLong,
    Cut,
    Internal,
}

impl Failure {
    fn reason(&self) -> String {
        match self {
            Self::KeyRefused => "key refused".into(),
            Self::ModelNotFound => "model not found".into(),
            Self::Unreachable => "unreachable".into(),
            Self::Timeout => "timeout".into(),
            Self::Status(status) => format!("error {status}"),
            Self::TooLong => "too long".into(),
            Self::Cut => "cut".into(),
            Self::Internal => "error".into(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum End {
    Done,
    /// By `chat-stop`, or because the client is gone.
    Stopped,
    Failed(Failure),
}

struct Running {
    conversation: String,
    cancel: CancellationToken,
}

struct Inner {
    settings: Settings,
    sink: Mutex<FileSink>,
    handle: Handle,
    deadlines: Deadlines,
    client: reqwest::Client,
    wire_limit: usize,
    running: Mutex<Option<Running>>,
}

/// The chat commands. Cheap to clone.
#[derive(Clone)]
pub struct Chat {
    inner: Arc<Inner>,
}

struct Slot {
    inner: Arc<Inner>,
}

impl Drop for Slot {
    fn drop(&mut self) {
        *lock(&self.inner.running) = None;
    }
}

pub(crate) fn event_error(reason: &str) -> String {
    json!({ "event": "error", "reason": reason }).to_string()
}

/// Only the canonical form (lower case, hyphenated) is accepted: an id becomes a file name, and
/// the braced, simple and URN spellings of one UUID would be three names for one conversation.
fn valid_id(id: &str) -> bool {
    Uuid::parse_str(id).is_ok_and(|u| u.hyphenated().to_string() == id)
}

impl Chat {
    /// The chat over the settings, storing its conversations under `data_dir`. Answers run on
    /// `handle`'s runtime, driven by the thread of the connection.
    ///
    /// # Errors
    /// Fails when the HTTP client cannot be built.
    pub fn new(
        settings: Settings,
        data_dir: &Path,
        handle: Handle,
        deadlines: Deadlines,
    ) -> Result<Self, reqwest::Error> {
        // No `.timeout()`: it covers the whole body and would cut a long answer. No redirects
        // and no proxy: the key goes only to the address that was checked.
        let client = reqwest::Client::builder()
            .connect_timeout(deadlines.connect)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()?;
        Ok(Self {
            inner: Arc::new(Inner {
                settings,
                sink: Mutex::new(FileSink::new(data_dir)),
                handle,
                deadlines,
                client,
                wire_limit: WIRE_LIMIT,
                running: Mutex::new(None),
            }),
        })
    }

    /// Read at most `limit` bytes from the provider for one answer; for the tests, since the
    /// default is [`WIRE_LIMIT`]. Call it before the chat is cloned.
    #[must_use]
    pub fn with_wire_limit(mut self, limit: usize) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.wire_limit = limit;
        }
        self
    }

    /// Run `chat-send` with `argument` (a JSON object). Every line to the client goes through
    /// `out`, which returns `false` when the client is gone.
    pub fn send(&self, argument: &str, out: &mut dyn FnMut(&str) -> bool) {
        self.inner.handle.block_on(self.send_async(argument, out));
    }

    async fn send_async(&self, argument: &str, out: &mut dyn FnMut(&str) -> bool) {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request {
            #[serde(default)]
            conversation: String,
            message: String,
        }
        let inner = &self.inner;
        let Ok(request) = serde_json::from_str::<Request>(argument) else {
            out(&event_error("invalid request"));
            return;
        };
        if request.message.trim().is_empty() {
            out(&event_error("empty message"));
            return;
        }
        let new = request.conversation.is_empty();
        let conversation = if new {
            Uuid::new_v4().to_string()
        } else {
            request.conversation.clone()
        };
        let Some(target) = Target::from_settings(&inner.settings.model()) else {
            out(&event_error("no model"));
            return;
        };
        let Some((slot, cancel)) = self.acquire(&conversation) else {
            out(&event_error("busy"));
            return;
        };
        // Read once the slot is held: a `chat-delete` cannot slip in between.
        let history = match self.history(&conversation, new) {
            Ok(history) => history,
            Err(reason) => {
                drop(slot);
                out(&event_error(reason));
                return;
            }
        };

        let mut last = history.iter().filter_map(|r| millis(r.time())).max();
        // A failed answer with no text leaves its user turn as the last record: sending the same
        // text again is a retry and answers that turn, it does not store the message a second
        // time. After a partial answer (`cut`, `timeout` after some text) the partial assistant
        // turn is the last record and stays part of the conversation, so the same text then is
        // a new message: the model sees user, partial answer, user.
        let retry = matches!(
            history.last(),
            Some(Record::Turn(t)) if t.role == Role::User && t.text == request.message
        );
        let user = Record::Turn(Turn {
            tool: SOURCE.into(),
            session: conversation.clone(),
            time: next_time(&mut last),
            folder: String::new(),
            branch: String::new(),
            role: Role::User,
            text: request.message,
        });
        if !retry && self.store(&user).is_err() {
            drop(slot);
            out(&event_error("cannot save"));
            return;
        }
        if !out(&json!({ "event": "start", "conversation": conversation }).to_string()) {
            return;
        }
        let Ok(records) = self.read(&conversation) else {
            drop(slot);
            out(&event_error("cannot read"));
            return;
        };
        let mut text = String::new();
        let end = match request_body(&target.model, &records) {
            Ok(body) => {
                self.stream_answer(&target, body, &cancel, &mut text, &mut |piece| {
                    out(&json!({ "event": "delta", "text": piece }).to_string())
                })
                .await
            }
            Err(_) => End::Failed(Failure::Internal),
        };
        // Stored before the last line is written, so a client that has seen the end can read
        // the whole conversation back.
        let stored = text.is_empty()
            || self
                .store(&Record::Turn(Turn {
                    tool: SOURCE.into(),
                    session: conversation.clone(),
                    time: next_time(&mut last),
                    folder: String::new(),
                    branch: String::new(),
                    role: Role::Assistant,
                    text,
                }))
                .is_ok();
        let line = match end {
            _ if !stored => event_error("cannot save"),
            End::Done => json!({ "event": "done" }).to_string(),
            End::Stopped => json!({ "event": "done", "stopped": true }).to_string(),
            End::Failed(f) => event_error(&f.reason()),
        };
        // The line only says how it ended: never the text, never what the provider said.
        eprintln!("chat: answer ended: {line}");
        // Free the slot before the last line: a client that sends again on seeing it must not
        // meet `busy`.
        drop(slot);
        out(&line);
    }

    /// The stored records of the conversation a request names: none for a new one, and the
    /// reason for one that is not there.
    fn history(&self, id: &str, new: bool) -> Result<Vec<Record>, &'static str> {
        if new {
            return Ok(Vec::new());
        }
        if !valid_id(id) {
            return Err("unknown conversation");
        }
        match self.read(id) {
            Ok(history) if !history.is_empty() => Ok(history),
            Ok(_) => Err("unknown conversation"),
            Err(_) => Err("cannot read"),
        }
    }

    /// The answer slot, or `None` while another answer runs.
    fn acquire(&self, conversation: &str) -> Option<(Slot, CancellationToken)> {
        let mut running = lock(&self.inner.running);
        if running.is_some() {
            return None;
        }
        let cancel = CancellationToken::new();
        *running = Some(Running {
            conversation: conversation.to_string(),
            cancel: cancel.clone(),
        });
        Some((
            Slot {
                inner: Arc::clone(&self.inner),
            },
            cancel,
        ))
    }

    fn read(&self, conversation: &str) -> io::Result<Vec<Record>> {
        lock(&self.inner.sink).read_session(SOURCE, conversation)
    }

    /// Appends one record and makes it durable (the session only, never the readers' progress).
    fn store(&self, record: &Record) -> io::Result<()> {
        use crate::records::Sink;
        let mut sink = lock(&self.inner.sink);
        sink.append(record)?;
        sink.flush_session(SOURCE, record.session())
    }

    /// One request to the provider and its answer, until it ends. The text so far is in `text`
    /// whatever the end. `emit` gets each piece and returns `false` when the client is gone.
    async fn stream_answer(
        &self,
        target: &Target,
        body: String,
        cancel: &CancellationToken,
        text: &mut String,
        emit: &mut dyn FnMut(&str) -> bool,
    ) -> End {
        let deadlines = self.inner.deadlines;
        let first_deadline = Instant::now() + deadlines.first_word;
        let send = target
            .request(&self.inner.client, "text/event-stream", body)
            .send();
        let response = tokio::select! {
            biased;
            () = cancel.cancelled() => return End::Stopped,
            r = timeout_at(first_deadline, send) => match r {
                Err(_) => return End::Failed(Failure::Timeout),
                // A connect that timed out is "unreachable", as in the Test call.
                Ok(Err(e)) if e.is_timeout() && !e.is_connect() => {
                    return End::Failed(Failure::Timeout);
                }
                Ok(Err(_)) => return End::Failed(Failure::Unreachable),
                Ok(Ok(response)) => response,
            },
        };
        let status = response.status();
        if !status.is_success() {
            return tokio::select! {
                biased;
                () = cancel.cancelled() => End::Stopped,
                failure = refusal(status, response) => End::Failed(failure),
            };
        }
        let mut events = match ServerEventsStream::from_response::<ServerEvent>(into_http(
            response,
            self.inner.wire_limit,
        )) {
            ServerEventsResponse::Events(events) => events,
            // A success that is no event stream is not an answer.
            ServerEventsResponse::Response(_) => {
                return End::Failed(Failure::Status(status.as_u16()));
            }
        };

        let mut got_chunk = false;
        let mut finished = false;
        loop {
            let wait = if got_chunk {
                Instant::now() + deadlines.silence
            } else {
                first_deadline
            };
            let next = tokio::select! {
                biased;
                () = cancel.cancelled() => return End::Stopped,
                r = timeout_at(wait, events.next()) => r,
            };
            // Before any text the failure has its own reason; after some, the answer is cut.
            let broken =
                |base: Failure| End::Failed(if text.is_empty() { base } else { Failure::Cut });
            let event = match next {
                Err(_) => return broken(Failure::Timeout),
                Ok(Some(Ok(event))) => event,
                // Some servers close after the last chunk without `[DONE]`; a `finish_reason`
                // says the answer was complete.
                Ok(None) if finished => return End::Done,
                // The wire cap is a cut, whatever the text holds.
                Ok(Some(Err(StreamingError::Stream(e)))) if e.is::<WireCap>() => {
                    return End::Failed(Failure::Cut);
                }
                Ok(Some(Err(_)) | None) => return broken(Failure::Unreachable),
            };
            got_chunk = true;
            if event.data.trim() == "[DONE]" {
                return End::Done;
            }
            // An event that is no chunk (a keep-alive, a vendor's note) says nothing about the
            // answer: skip it.
            let Ok(chunk) = serde_json::from_str::<Chunk>(&event.data) else {
                continue;
            };
            let Some(choice) = chunk.choices.into_iter().next() else {
                continue;
            };
            finished |= choice.finish_reason.is_some();
            if let Some(piece) = choice.delta.content.filter(|p| !p.is_empty()) {
                if text.len() + piece.len() > TEXT_LIMIT {
                    return End::Failed(Failure::Cut);
                }
                text.push_str(&piece);
                if !emit(&piece) {
                    return End::Stopped;
                }
            }
        }
    }

    /// Stop the answer of `conversation`. `true` when one was running.
    #[must_use]
    pub fn stop(&self, conversation: &str) -> bool {
        match &*lock(&self.inner.running) {
            Some(r) if r.conversation == conversation => {
                r.cancel.cancel();
                true
            }
            _ => false,
        }
    }

    /// The JSON line for `chat-list`: the conversations, the newest first.
    #[must_use]
    pub fn list(&self) -> String {
        match lock(&self.inner.sink).list_sessions(SOURCE) {
            Ok(sessions) => {
                let conversations: Vec<_> = sessions
                    .iter()
                    .map(|s| {
                        json!({
                            "conversation": s.session,
                            "title": s.first_user_text.chars().take(TITLE_CHARS).collect::<String>(),
                            "time": s.last_time,
                        })
                    })
                    .collect();
                json!({ "conversations": conversations }).to_string()
            }
            Err(_) => error_response("cannot read"),
        }
    }

    /// The JSON lines for `chat-get`, one per record, oldest first.
    #[must_use]
    pub fn get(&self, conversation: &str) -> Vec<String> {
        let records = match valid_id(conversation).then(|| self.read(conversation)) {
            Some(Ok(records)) if !records.is_empty() => records,
            Some(Err(_)) => return vec![error_response("cannot read")],
            _ => return vec![error_response("unknown conversation")],
        };
        records
            .iter()
            .filter_map(|r| match r {
                Record::Turn(t) => Some(json!({
                    "kind": "turn",
                    "role": match t.role { Role::User => "user", Role::Assistant => "assistant" },
                    "time": t.time,
                    "text": t.text,
                })),
                Record::ToolCall(c) => Some(json!({
                    "kind": "toolcall",
                    "time": c.time,
                    "call_id": c.call_id,
                    "name": c.name,
                    "arguments": c.arguments,
                })),
                Record::ToolResult(r) => Some(json!({
                    "kind": "toolresult",
                    "time": r.time,
                    "call_id": r.call_id,
                    "text": r.text,
                })),
                Record::Mark(_) => None,
            })
            .map(|v| v.to_string())
            .collect()
    }

    /// The JSON line for `chat-delete` of one conversation or `all`. A conversation that is
    /// answering is not deleted.
    #[must_use]
    pub fn delete(&self, which: &str) -> String {
        // The guard is held on purpose, to the end: no answer can start for the conversation
        // while it is deleted.
        let running = lock(&self.inner.running);
        if running
            .as_ref()
            .is_some_and(|r| which == "all" || r.conversation == which)
        {
            return error_response("busy");
        }
        let mut sink = lock(&self.inner.sink);
        let result = if which == "all" {
            sink.list_sessions(SOURCE).and_then(|sessions| {
                sink.delete_source(SOURCE)?;
                Ok(sessions.len())
            })
        } else if valid_id(which) {
            sink.read_session(SOURCE, which).and_then(|records| {
                sink.delete_session(SOURCE, which)?;
                Ok(usize::from(!records.is_empty()))
            })
        } else {
            return error_response("unknown conversation");
        };
        match result {
            Ok(n) => json!({ "deleted": n }).to_string(),
            Err(_) => error_response("cannot delete"),
        }
    }
}

/// What a refusal (any status that is not a success) comes to. Only the status is used, and a
/// 400 body is read for one question: does it say the conversation is too long.
async fn refusal(status: StatusCode, response: reqwest::Response) -> Failure {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Failure::KeyRefused,
        StatusCode::NOT_FOUND => Failure::ModelNotFound,
        StatusCode::BAD_REQUEST => {
            let body = timeout(ERROR_BODY_DEADLINE, read_capped(response, ERROR_BODY_LIMIT))
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default();
            let body = String::from_utf8_lossy(&body);
            if body.contains("context_length_exceeded") {
                Failure::TooLong
            } else {
                Failure::Status(status.as_u16())
            }
        }
        _ => Failure::Status(status.as_u16()),
    }
}

/// A record time strictly after `last` (milliseconds since the epoch) and after the clock's
/// reading, as RFC 3339 UTC: times only grow, and a turn that repeats an earlier one is not
/// dropped as a replay.
fn next_time(last: &mut Option<i64>) -> String {
    let now =
        i64::try_from(OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).unwrap_or(0);
    let at = last.map_or(now, |l| now.max(l + 1));
    *last = Some(at);
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(at) * 1_000_000)
        .ok()
        .and_then(|t| t.format(&Rfc3339).ok())
        .unwrap_or_default()
}

/// Milliseconds since the epoch of an RFC 3339 time; `None` for a time that does not parse,
/// which then does not count when the next time is chosen.
fn millis(time: &str) -> Option<i64> {
    let t = OffsetDateTime::parse(time, &Rfc3339).ok()?;
    i64::try_from(t.unix_timestamp_nanos() / 1_000_000).ok()
}

#[cfg(test)]
// A failed `unwrap` in a test is a failed test.
#[allow(clippy::unwrap_used)]
mod tests {
    use connectors::compare_times;

    use super::*;

    #[test]
    fn the_next_time_is_one_millisecond_after_the_last() {
        // Far ahead of the clock, so only the `last + 1` rule can give these.
        let ahead = 4_000_000_000_000;
        let mut last = Some(ahead);
        let (a, b) = (next_time(&mut last), next_time(&mut last));
        assert_eq!(millis(&a), Some(ahead + 1));
        assert_eq!(millis(&b), Some(ahead + 2));
        assert_eq!(compare_times(&b, &a), std::cmp::Ordering::Greater);
        assert_eq!(millis("not a time"), None);
    }

    #[test]
    fn ids_are_lower_case_hyphenated_uuids() {
        assert!(valid_id(&Uuid::new_v4().to_string()));
        assert!(!valid_id("../x"));
        assert!(!valid_id(&Uuid::new_v4().to_string().to_uppercase()));
        assert!(!valid_id(&Uuid::new_v4().simple().to_string()));
    }
}
