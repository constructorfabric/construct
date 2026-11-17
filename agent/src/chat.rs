//! The chat loop behind `chat-send`, `chat-stop`, `chat-list`, `chat-get` and `chat-delete`.
//!
//! A conversation is a session of the source `chat` in the record store; `chat-send` streams the
//! provider's answer to the client as JSON lines. The history never travels over the socket;
//! the loop only reads the person's profile from Construct (see below), writes nothing there
//! (no `manage-facts`) and never calls `save_progress`.
//!
//! Before each answer the person's facts are put in the conversation as a `recall_facts` tool
//! call and its result, when they differ from the last result there (see `recall`).
//!
//! When a model turn asks for tools, the calls are stored, run one by one in the order given and
//! answered in the conversation, and the model is asked again, until a turn has no call. One answer
//! runs at most [`MAX_CALLS`] calls. The tools themselves are given to the chat (see
//! [`Chat::with_tools`]): a fixed list, offered to the model in every request; a call to a name
//! that is not in it is answered with an error text. A result is cut at 64 KiB, note included.
//!
//! The request body is written from typed structs in a fixed field order, never through a
//! `serde_json::Value` (it sorts keys), with tool-call arguments as the stored text, so a request
//! starts with the bytes of the one before it and the provider's prompt cache can use that.
//!
//! No log line, event or error holds the key, a message text or a fact: failures are named by reason
//! only, and the provider's error body is read for one question and dropped.

use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use connectors::{Author, Record, Role, ToolCall, ToolResult, Turn};
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

use crate::host::Connection;
use crate::ipc::error_response;
use crate::model::{ANSWER_LIMIT, Target, read_capped};
use crate::records::FileSink;
use crate::settings::Settings;
use crate::sync::lock;

mod recall;
pub mod tool;
mod wire;

use tool::{ChatTools, limited};
use wire::{Calls, Chunk, PendingCall, WireCap, into_http, request_body};

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
/// Most tool calls one answer runs. The answer ends with [`CALL_LIMIT_TEXT`] instead of running more.
const MAX_CALLS: usize = 8;
const CALL_LIMIT_TEXT: &str = "I stopped after 8 tool calls.";
/// The result of a call that was cut short by `chat-stop`.
const STOPPED_RESULT: &str = "stopped by the user";
/// A client that does not read for this long is gone.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// The one system instruction. It never changes, because the provider's prompt cache needs the
/// same start for every request.
pub const INSTRUCTION: &str = "You are the chat of a person's Construct, the assistant that lives in their Construct Browser. \
You cannot save anything yourself. Whatever the user tells you is remembered later by the system, on its own, \
unless the `recall_facts` tool returns the error that memory is off. So when the user tells you something worth \
keeping, say that you will remember it, and never say that you have saved it. If `recall_facts` returns the error \
that memory is off, say that nothing is remembered. Answer in the language of the user.";

/// What the client is told while an answer runs, besides how it ends.
enum Piece<'a> {
    /// A piece of the answer text.
    Text(&'a str),
    /// A tool call starts: its short line.
    Tool(&'a str),
}

impl Piece<'_> {
    fn event(&self) -> String {
        match self {
            Self::Text(text) => json!({ "event": "delta", "text": text }),
            Self::Tool(line) => json!({ "event": "tool", "line": line }),
        }
        .to_string()
    }
}

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
    CannotSave,
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
            Self::CannotSave => "cannot save".into(),
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

/// Where the chat finds the Construct host: empty until the host is ready. Clones share it.
#[derive(Clone, Default)]
pub struct Link(Arc<Mutex<Option<Connection>>>);

impl Link {
    pub fn set(&self, connection: Connection) {
        *lock(&self.0) = Some(connection);
    }

    pub(crate) fn get(&self) -> Option<Connection> {
        lock(&self.0).clone()
    }
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
    link: Link,
    tools: ChatTools,
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
                link: Link::default(),
                tools: ChatTools::default(),
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

    /// Read the facts from the host `link` names; without it every answer starts with no facts.
    /// Call it before the chat is cloned.
    #[must_use]
    pub fn with_link(mut self, link: Link) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.link = link;
        }
        self
    }

    /// Offer `tools` to the model and run its calls; without any, no list is sent and every call is
    /// answered with an unknown-tool error. Call it before the chat is cloned.
    ///
    /// The chat puts no deadline on a call: a tool bounds its own run time. A call runs on a
    /// thread that Stop cannot end, so after a Stop it goes on until it returns, and the next answer
    /// does not wait for it.
    #[must_use]
    pub fn with_tools(mut self, tools: ChatTools) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.tools = tools;
        }
        self
    }

    /// Run `chat-send` with `argument` (a JSON object). Every line to the client goes through
    /// `out`, which returns `false` when the client is gone.
    pub fn send(&self, argument: &str, out: &mut dyn FnMut(&str) -> bool) {
        self.inner.handle.block_on(self.send_async(argument, out));
    }

    // One sequence of steps for one answer; split it when a part grows its own logic.
    #[allow(clippy::too_many_lines)]
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
        // A `recall_facts` pair stored after that user turn does not make it another message.
        let retry = matches!(
            history.iter().rfind(|r| !matches!(r, Record::ToolCall(_) | Record::ToolResult(_))),
            Some(Record::Turn(t)) if t.role == Role::User && t.text == request.message
        );
        let user = Record::Turn(Turn {
            tool: SOURCE.into(),
            session: conversation.clone(),
            time: next_time(&mut last),
            folder: String::new(),
            branch: String::new(),
            role: Role::User,
            author: Author::Unknown,
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
        if self
            .recall(&conversation, &history, &mut last)
            .await
            .is_err()
        {
            drop(slot);
            out(&event_error("cannot save"));
            return;
        }
        let Ok(records) = self.read(&conversation) else {
            drop(slot);
            out(&event_error("cannot read"));
            return;
        };
        let end = self
            .answer(
                &target,
                &conversation,
                records,
                &cancel,
                &mut last,
                &mut |piece| out(&piece.event()),
            )
            .await;
        // Everything is stored before the last line is written, so a client that has seen the end
        // can read the whole conversation back.
        let line = match end {
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

    /// Store the `recall_facts` call and its result after the user's turn when the text of the
    /// result differs from the last one in `history` (none yet counts as no facts).
    async fn recall(
        &self,
        conversation: &str,
        history: &[Record],
        last: &mut Option<i64>,
    ) -> io::Result<()> {
        let inner = &self.inner;
        let text = if inner.settings.collection().enabled {
            recall::facts(&inner.client, inner.link.get().as_ref()).await
        } else {
            recall::MEMORY_OFF.to_string()
        };
        if text == recall::last_result(history) {
            return Ok(());
        }
        let call_id = format!("call_{}", Uuid::new_v4().simple());
        self.store(&Record::ToolCall(ToolCall {
            tool: SOURCE.into(),
            session: conversation.into(),
            time: next_time(last),
            call_id: call_id.clone(),
            name: recall::NAME.into(),
            arguments: "{}".into(),
        }))?;
        self.store_result(conversation, last, &call_id, &text)
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

    /// The answer: one request to the provider per model turn, and between two turns the tool calls
    /// of the first run. Every turn's text and every call and result are stored as they come.
    /// `emit` gets each piece of text and each call that starts, and returns `false` when the
    /// client is gone.
    async fn answer(
        &self,
        target: &Target,
        conversation: &str,
        mut records: Vec<Record>,
        cancel: &CancellationToken,
        last: &mut Option<i64>,
        emit: &mut dyn FnMut(Piece<'_>) -> bool,
    ) -> End {
        let mut run = 0;
        loop {
            let Ok(body) = request_body(&target.model, &records, self.inner.tools.listing()) else {
                return End::Failed(Failure::Internal);
            };
            let mut text = String::new();
            let mut calls = Calls::default();
            let end = self
                .stream_answer(target, body, cancel, &mut text, &mut calls, emit)
                .await;
            if !text.is_empty() && self.store_text(conversation, last, text).is_err() {
                return End::Failed(Failure::CannotSave);
            }
            if end != End::Done {
                return end;
            }
            let mut calls = calls.finish();
            if calls.is_empty() {
                return End::Done;
            }
            // A call with no name cannot be answered or replayed: the turn is broken.
            if calls.iter().any(|call| call.name.is_empty()) {
                return End::Failed(Failure::Cut);
            }
            // The calls over the limit are not stored and not run.
            let over = calls.len() > MAX_CALLS - run;
            calls.truncate(MAX_CALLS - run);
            run += calls.len();
            if let Err(end) = self
                .run_calls(conversation, calls, cancel, last, emit)
                .await
            {
                return end;
            }
            if over {
                if self
                    .store_text(conversation, last, CALL_LIMIT_TEXT.into())
                    .is_err()
                {
                    return End::Failed(Failure::CannotSave);
                }
                return if emit(Piece::Text(CALL_LIMIT_TEXT)) {
                    End::Done
                } else {
                    End::Stopped
                };
            }
            records = match self.read(conversation) {
                Ok(records) => records,
                Err(_) => return End::Failed(Failure::Internal),
            };
        }
    }

    fn store_text(
        &self,
        conversation: &str,
        last: &mut Option<i64>,
        text: String,
    ) -> io::Result<()> {
        self.store(&Record::Turn(Turn {
            tool: SOURCE.into(),
            session: conversation.into(),
            time: next_time(last),
            folder: String::new(),
            branch: String::new(),
            role: Role::Assistant,
            author: Author::Unknown,
            text,
        }))
    }

    /// Store `calls`, then run them in order, storing each result. When the answer is stopped, every
    /// call that has no result yet gets [`STOPPED_RESULT`] and the error is `End::Stopped`. The
    /// client is told of each call as it starts, except the facts call, which the window never shows.
    async fn run_calls(
        &self,
        conversation: &str,
        calls: Vec<PendingCall>,
        cancel: &CancellationToken,
        last: &mut Option<i64>,
        emit: &mut dyn FnMut(Piece<'_>) -> bool,
    ) -> Result<(), End> {
        let calls: Vec<ToolCall> = calls
            .into_iter()
            .map(|call| ToolCall {
                tool: SOURCE.into(),
                session: conversation.into(),
                time: String::new(),
                call_id: if call.id.is_empty() {
                    format!("call_{}", Uuid::new_v4().simple())
                } else {
                    call.id
                },
                name: call.name,
                // A call with no arguments is sent back as an empty object, as providers expect.
                arguments: if call.arguments.is_empty() {
                    "{}".into()
                } else {
                    call.arguments
                },
            })
            .collect();
        for call in &calls {
            let call = ToolCall {
                time: next_time(last),
                ..call.clone()
            };
            if self.store(&Record::ToolCall(call)).is_err() {
                return Err(End::Failed(Failure::CannotSave));
            }
        }
        for (done, call) in calls.iter().enumerate() {
            // A client that is gone ends the answer as a stop does.
            let told = call.name == recall::NAME
                || emit(Piece::Tool(
                    &self.inner.tools.line(&call.name, &call.arguments),
                ));
            let outcome = if told {
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => None,
                    result = self.run_tool(&call.name, &call.arguments) => Some(result),
                }
            } else {
                None
            };
            let Some(result) = outcome else {
                // A stop answers this call and every call after it.
                for call in &calls[done..] {
                    self.store_result(conversation, last, &call.call_id, STOPPED_RESULT)
                        .map_err(|_| End::Failed(Failure::CannotSave))?;
                }
                return Err(End::Stopped);
            };
            let text = limited(result.unwrap_or_else(|error| format!("Error: {error}")));
            self.store_result(conversation, last, &call.call_id, &text)
                .map_err(|_| End::Failed(Failure::CannotSave))?;
        }
        Ok(())
    }

    fn store_result(
        &self,
        conversation: &str,
        last: &mut Option<i64>,
        call_id: &str,
        text: &str,
    ) -> io::Result<()> {
        self.store(&Record::ToolResult(ToolResult {
            tool: SOURCE.into(),
            session: conversation.into(),
            time: next_time(last),
            call_id: call_id.into(),
            text: text.into(),
        }))
    }

    /// One tool call, on a thread of its own so that a stop does not wait for it.
    async fn run_tool(&self, name: &str, arguments: &str) -> Result<String, String> {
        let Some(tool) = self.inner.tools.find(name) else {
            return Err(format!("unknown tool {name}"));
        };
        let arguments = arguments.to_string();
        self.inner
            .handle
            .spawn_blocking(move || tool.run(&arguments))
            .await
            .unwrap_or_else(|_| Err("the tool failed".into()))
    }

    /// One request to the provider and its answer, until it ends. The text so far is in `text`
    /// whatever the end, and the tool calls the model asked for in `calls`. `emit` gets each piece
    /// of text and returns `false` when the client is gone.
    async fn stream_answer(
        &self,
        target: &Target,
        body: String,
        cancel: &CancellationToken,
        text: &mut String,
        calls: &mut Calls,
        emit: &mut dyn FnMut(Piece<'_>) -> bool,
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
            for fragment in choice.delta.tool_calls {
                if calls.push(fragment, TEXT_LIMIT).is_err() {
                    return End::Failed(Failure::Cut);
                }
            }
            if let Some(piece) = choice.delta.content.filter(|p| !p.is_empty()) {
                if text.len() + piece.len() > TEXT_LIMIT {
                    return End::Failed(Failure::Cut);
                }
                text.push_str(&piece);
                if !emit(Piece::Text(&piece)) {
                    return End::Stopped;
                }
            }
            // A `finish_reason` ends the model turn: the usage chunk and `[DONE]` that may follow
            // are not waited for, so a server that holds the connection open is no matter. Only
            // `stop` and `tool_calls` say the turn was whole; for any other reason (`length`,
            // `content_filter`, a vendor's own) the arguments of its calls may not be, so none is kept.
            if let Some(reason) = choice.finish_reason.filter(|r| !r.is_empty()) {
                let whole = matches!(reason.as_str(), "stop" | "tool_calls" | "function_call");
                return if !whole && !calls.is_empty() {
                    End::Failed(Failure::Cut)
                } else {
                    End::Done
                };
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

    /// The JSON lines for `chat-get`, one per record, oldest first. The tool records are marked
    /// `hidden`: the window does not show them. A call that is not the facts call also gets an entry
    /// of its own, a turn of the role `tool` whose text is its line, made from the stored arguments
    /// so it reads the same after a restart.
    #[must_use]
    pub fn get(&self, conversation: &str) -> Vec<String> {
        let records = match valid_id(conversation).then(|| self.read(conversation)) {
            Some(Ok(records)) if !records.is_empty() => records,
            Some(Err(_)) => return vec![error_response("cannot read")],
            _ => return vec![error_response("unknown conversation")],
        };
        records
            .iter()
            .flat_map(|r| match r {
                Record::Turn(t) => vec![json!({
                    "kind": "turn",
                    "role": match t.role { Role::User => "user", Role::Assistant => "assistant" },
                    "time": t.time,
                    "text": t.text,
                })],
                Record::ToolCall(c) => {
                    let mut entries = vec![json!({
                        "kind": "toolcall",
                        "hidden": true,
                        "time": c.time,
                        "call_id": c.call_id,
                        "name": c.name,
                        "arguments": c.arguments,
                    })];
                    if c.name != recall::NAME {
                        entries.push(json!({
                            "kind": "turn",
                            "role": "tool",
                            "time": c.time,
                            "text": self.inner.tools.line(&c.name, &c.arguments),
                        }));
                    }
                    entries
                }
                Record::ToolResult(r) => vec![json!({
                    "kind": "toolresult",
                    "hidden": true,
                    "time": r.time,
                    "call_id": r.call_id,
                    "text": r.text,
                })],
                Record::Mark(_) => Vec::new(),
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
