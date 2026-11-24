//! The chat loop behind `chat-send`, `chat-stop`, `chat-list`, `chat-get` and `chat-delete`.
//!
//! A conversation is a session of the source `chat` in the host's records (see [`Records`]);
//! `chat-send` streams the provider's answer to the client as JSON lines. The history never
//! travels over the socket; the loop only reads the person's profile from the host's facts (see
//! below) and writes nothing there.
//!
//! Before each answer the person's facts are put in the conversation as a `recall_facts` tool
//! call and its result, when they differ from the last result there (see `recall`).
//!
//! When a model turn asks for tools, the calls are stored, run one by one in the order given and
//! answered in the conversation, and the model is asked again, until a turn has no call or the user
//! stops the answer. The tools themselves are given to the chat (see
//! [`Chat::with_tools`]): a fixed list, offered to the model in every request; a call to a name
//! that is not in it is answered with an error text. A result is cut at 64 KiB, note included.
//!
//! The list may hold the `run_subagent` tool (see [`subagent`]): a call to it runs a second,
//! separate conversation of the same model inside the call, with the other tools of the list. The
//! bytes sent to the provider and received from it by an answer, sub-agents included, are counted
//! and reported once the answer ends (see [`Chat::with_usage`]).
//!
//! The request body is written from typed structs in a fixed field order, never through a
//! `serde_json::Value` (it sorts keys), with tool-call arguments as the stored text, so a request
//! starts with the bytes of the one before it and the provider's prompt cache can use that.
//!
//! No log line, event or error holds the key, a message text or a fact: failures are named by reason
//! only, and the provider's error body is read for one question and dropped.

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use connectors::{Author, Record, Role, ToolCall, ToolResult, Turn};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::runtime::Handle;
use tokio::time::{Instant, timeout, timeout_at};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::model::{
    ANSWER_LIMIT, ModelSource, Target, read_capped, refuses_tools_with_reasoning, without_reasoning,
};
use crate::recall::{self, Facts};
use crate::records::Records;
use crate::sse::{Events, StreamError, is_event_stream};
use crate::subagent;
use crate::sync::lock;
use crate::tool::{ChatTools, limited};
pub use crate::usage::{Counter, Usage};
use crate::wire::{Calls, Chunk, PendingCall, request_body};

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
pub const TITLE_CHARS: usize = 60;
/// The result of a call that was cut short by `chat-stop`.
pub(crate) const STOPPED_RESULT: &str = "stopped by the user";
/// How long the erasure waits for a running answer to end after it was stopped.
const ERASE_WAIT: Duration = Duration::from_secs(30);
/// A client that does not read for this long is gone.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// The one system instruction. It never changes, because the provider's prompt cache needs the
/// same start for every request.
pub const INSTRUCTION: &str = "You are the chat of a person's Construct, the assistant that lives in their Construct Browser. \
You cannot save anything yourself. Whatever the user tells you is remembered later by the system, on its own, \
unless the `recall_facts` tool returns the error that memory is off. So when the user tells you something worth \
keeping, say that you will remember it, and never say that you have saved it. If `recall_facts` returns the error \
that memory is off, say that nothing is remembered. \
You have a workspace folder with the user's files, and the files are text. You work in it with `Glob`, `Read`, \
`Write` and `Edit`. `Read` shows every line with its number and a tab: the numbers are not part of the file, so \
never put them in the text you give to `Edit` or `Write`. When a tool returns an error, say what the error says, \
and never say that a change was made when it failed. Answer in the language of the user.";

/// What the client is told while an answer runs, besides how it ends.
pub enum Piece<'a> {
    /// A piece of the answer text.
    Text(&'a str),
    /// A tool call starts: its short line.
    Tool(&'a str),
}

impl Piece<'_> {
    pub(crate) fn event(&self) -> String {
        match self {
            Self::Text(text) => json!({ "event": "delta", "text": text }),
            Self::Tool(line) => json!({ "event": "tool", "line": line }),
        }
        .to_string()
    }
}

/// What the requests of one answer share: the model, the stop and the count of bytes.
#[derive(Clone, Copy)]
pub struct Run<'a> {
    pub target: &'a Target,
    pub cancel: &'a CancellationToken,
    pub usage: &'a Counter,
    /// The tool calls of this model call that run on threads of their own.
    pub tools: &'a ToolCount,
}

/// The calls of a turn as records of `session`. A call with no id gets one, and one with no
/// arguments is sent back as an empty object, as providers expect.
pub(crate) fn tool_calls(calls: Vec<PendingCall>, session: &str) -> Vec<ToolCall> {
    calls
        .into_iter()
        .map(|call| ToolCall {
            tool: SOURCE.into(),
            session: session.into(),
            time: String::new(),
            call_id: if call.id.is_empty() {
                format!("call_{}", Uuid::new_v4().simple())
            } else {
                call.id
            },
            name: call.name,
            arguments: if call.arguments.is_empty() {
                "{}".into()
            } else {
                call.arguments
            },
        })
        .collect()
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
pub enum Failure {
    KeyRefused,
    ModelNotFound,
    Unreachable,
    Timeout,
    Status(u16),
    TooLong,
    /// The API refused function tools next to a reasoning effort; it is asked again without one.
    ToolsWithReasoning,
    Cut,
    CannotSave,
    Internal,
}

impl Failure {
    /// The plain text of the reason, which is what the client is told.
    #[must_use]
    pub fn reason(&self) -> String {
        match self {
            Self::KeyRefused => "key refused".into(),
            Self::ModelNotFound => "model not found".into(),
            Self::Unreachable => "unreachable".into(),
            Self::Timeout => "timeout".into(),
            Self::Status(status) => format!("error {status}"),
            Self::ToolsWithReasoning => "error 400".into(),
            Self::TooLong => "too long".into(),
            Self::Cut => "cut".into(),
            Self::CannotSave => "cannot save".into(),
            Self::Internal => "error".into(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum End {
    Done,
    /// By `chat-stop`, or because the client is gone.
    Stopped,
    Failed(Failure),
}

/// One model call that goes: a chat answer or a run of the host's own.
struct Running {
    id: u64,
    conversation: String,
    cancel: CancellationToken,
    /// The time of the user turn this call answers, once it is stored.
    turn: Option<String>,
}

struct Inner {
    source: Arc<dyn ModelSource>,
    sink: Mutex<Box<dyn Records>>,
    handle: Handle,
    deadlines: Deadlines,
    client: reqwest::Client,
    wire_limit: usize,
    facts: Arc<dyn Facts>,
    tools: ChatTools,
    /// The tools a sub-agent has: the list without the sub-agent tool.
    sub_tools: ChatTools,
    usage: Option<Arc<dyn Fn(Usage) + Send + Sync>>,
    /// Every model call that goes, in any number.
    running: Mutex<Vec<Running>>,
    next_call: AtomicU64,
    /// Held while a message is checked against the stored turns and its own turn is stored, so
    /// that two messages of one conversation are told apart and stored one after the other.
    intake: Mutex<()>,
    /// The time of the last record stored for each conversation: the times of one conversation
    /// only grow, whichever call stores.
    clock: Mutex<HashMap<String, i64>>,
}

/// The chat commands. Cheap to clone.
#[derive(Clone)]
pub struct Chat {
    inner: Arc<Inner>,
}

/// The tool calls of one model call that run on threads of their own. A stopped call leaves the
/// one it was waiting for running, so what that call writes lands later: whoever must see it
/// land waits on the count of its own call. Clones share the count.
#[derive(Clone, Default)]
pub struct ToolCount(Arc<AtomicUsize>);

impl ToolCount {
    /// Waits until none is left, at most `limit`. Answers whether none is.
    #[must_use]
    pub fn wait(&self, limit: Duration) -> bool {
        let end = std::time::Instant::now() + limit;
        while self.0.load(Ordering::SeqCst) > 0 {
            if std::time::Instant::now() >= end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }
}

/// One tool call that runs; it is counted until this is dropped, a panic of the tool included.
struct InFlight(Arc<AtomicUsize>);

impl InFlight {
    fn start(count: &ToolCount) -> Self {
        count.0.fetch_add(1, Ordering::SeqCst);
        Self(Arc::clone(&count.0))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The place of one model call among the calls that go; it is free when dropped.
pub struct Slot {
    inner: Arc<Inner>,
    id: u64,
}

impl Drop for Slot {
    fn drop(&mut self) {
        lock(&self.inner.running).retain(|r| r.id != self.id);
    }
}

#[must_use]
pub fn event_error(reason: &str) -> String {
    json!({ "event": "error", "reason": reason }).to_string()
}

/// The JSON line for a refused request; `message` is a fixed text.
fn error_response(message: &str) -> String {
    json!({ "error": message }).to_string()
}

/// The facts of a chat that has none: the memory is on and the host is never ready.
struct NoFacts;

impl Facts for NoFacts {
    fn enabled(&self) -> bool {
        true
    }

    fn profile<'a>(&'a self, _: &'a reqwest::Client) -> recall::ProfileRead<'a> {
        Box::pin(async { Ok(None) })
    }
}

/// Only the canonical form (lower case, hyphenated) is accepted: an id becomes a file name, and
/// the braced, simple and URN spellings of one UUID would be three names for one conversation.
fn valid_id(id: &str) -> bool {
    Uuid::parse_str(id).is_ok_and(|u| u.hyphenated().to_string() == id)
}

impl Chat {
    /// The chat over the model `source` names, storing its conversations in `records`. Answers run
    /// on `handle`'s runtime, driven by the thread of the connection.
    ///
    /// # Errors
    /// Fails when the HTTP client cannot be built.
    pub fn new(
        source: Arc<dyn ModelSource>,
        records: Box<dyn Records>,
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
                source,
                sink: Mutex::new(records),
                handle,
                deadlines,
                client,
                wire_limit: WIRE_LIMIT,
                facts: Arc::new(NoFacts),
                tools: ChatTools::default(),
                sub_tools: ChatTools::default(),
                usage: None,
                running: Mutex::new(Vec::new()),
                next_call: AtomicU64::new(0),
                intake: Mutex::new(()),
                clock: Mutex::new(HashMap::new()),
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

    /// Read the facts from `facts`; without it every answer starts with no facts. Call it before
    /// the chat is cloned.
    #[must_use]
    pub fn with_facts(mut self, facts: Arc<dyn Facts>) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.facts = facts;
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
            inner.sub_tools = tools.without(subagent::NAME);
            inner.tools = tools;
        }
        self
    }

    /// Tell `report` the bytes an answer sent and received once it ends, a stopped or failed one
    /// too, with those of its sub-agents counted in. Call it before the chat is cloned.
    #[must_use]
    pub fn with_usage(mut self, report: impl Fn(Usage) + Send + Sync + 'static) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.usage = Some(Arc::new(report));
        }
        self
    }

    /// The runtime the answers run on.
    #[must_use]
    pub fn handle(&self) -> &Handle {
        &self.inner.handle
    }

    /// The model the source names now, checked; `None` when there is none to call.
    #[must_use]
    pub fn target(&self) -> Option<Target> {
        Target::from_settings(&self.inner.source.model())
    }

    /// Tell the usage reporter, if there is one, the bytes of a run that ended.
    pub fn report(&self, usage: Usage) {
        if let Some(report) = &self.inner.usage {
            report(usage);
        }
    }

    /// Run `chat-send` with `argument` (a JSON object). Every line to the client goes through
    /// `out`, which returns `false` when the client is gone.
    pub fn send(&self, argument: &str, out: &mut dyn FnMut(&str) -> bool) {
        self.inner
            .handle
            .block_on(self.send_on_runtime(argument, CancellationToken::new(), out));
    }

    /// Run an answer on an already driven Tokio runtime with a host-owned stop token.
    // One sequence of steps for one answer; split it when a part grows its own logic.
    #[allow(clippy::too_many_lines)]
    pub async fn send_on_runtime(
        &self,
        argument: &str,
        cancel: CancellationToken,
        out: &mut dyn FnMut(&str) -> bool,
    ) {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request {
            #[serde(default)]
            conversation: String,
            message: String,
        }
        // The desktop transport counts the command prefix as part of the request line.
        if argument.len() > MAX_REQUEST - "chat-send ".len() {
            out(&event_error("request too large"));
            return;
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
        let Some(target) = Target::from_settings(&inner.source.model()) else {
            out(&event_error("no model"));
            return;
        };
        let slot = self.acquire_with_stop(&conversation, &cancel);
        // One message at a time is checked and stored: the second sees the first's turn.
        let intake = lock(&inner.intake);
        // Read once the slot is held: a `chat-delete` cannot slip in between.
        let history = match self.history(&conversation, new) {
            Ok(history) => history,
            Err(reason) => {
                drop(intake);
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
        // A turn that another call is answering right now is not a failed one: the same text is
        // then a new message.
        let retried = match history
            .iter()
            .rfind(|r| !matches!(r, Record::ToolCall(_) | Record::ToolResult(_)))
        {
            Some(Record::Turn(t)) if t.role == Role::User && t.text == request.message => {
                Some(t.time.clone())
            }
            _ => None,
        }
        .filter(|time| !self.answering(&conversation, time));
        let user = Record::Turn(Turn {
            tool: SOURCE.into(),
            session: conversation.clone(),
            time: next_time(&mut last),
            folder: String::new(),
            branch: String::new(),
            role: Role::User,
            author: Author::Unknown,
            text: request.message,
            tools: Vec::new(),
        });
        let turn = match retried {
            Some(time) => Ok(time),
            None => self.store(&user),
        };
        let Ok(turn) = turn else {
            drop(intake);
            drop(slot);
            out(&event_error("cannot save"));
            return;
        };
        self.claim(slot.id, turn);
        drop(intake);
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
        let usage = Counter::default();
        let tools = ToolCount::default();
        let end = self
            .answer(
                Run {
                    target: &target,
                    cancel: &cancel,
                    usage: &usage,
                    tools: &tools,
                },
                &conversation,
                records,
                &mut last,
                &mut |piece| out(&piece.event()),
            )
            .await;
        let usage = usage.total();
        if let Some(report) = &inner.usage {
            report(usage);
        }
        // Everything is stored before the last line is written, so a client that has seen the end
        // can read the whole conversation back.
        let line = match end {
            End::Done => json!({ "event": "done" }).to_string(),
            End::Stopped => json!({ "event": "done", "stopped": true }).to_string(),
            End::Failed(f) => event_error(&f.reason()),
        };
        // The line only says how it ended: never the text, never what the provider said.
        eprintln!(
            "chat: answer ended: {line}, sent {} bytes, received {} bytes",
            usage.sent, usage.received
        );
        // Free the slot before the last line: a client that deletes the conversation on seeing it
        // must not meet `busy`.
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
        let text = if inner.facts.enabled() {
            recall::facts(&inner.client, inner.facts.as_ref()).await
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
        }))
        .map(|_| ())?;
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

    /// A place for a new model call named `conversation`, with its own stop. There is no limit
    /// on the number of calls.
    #[must_use]
    pub fn acquire(&self, conversation: &str) -> (Slot, CancellationToken) {
        let cancel = CancellationToken::new();
        let slot = self.acquire_with_stop(conversation, &cancel);
        (slot, cancel)
    }

    fn acquire_with_stop(&self, conversation: &str, cancel: &CancellationToken) -> Slot {
        let id = self.inner.next_call.fetch_add(1, Ordering::SeqCst);
        lock(&self.inner.running).push(Running {
            id,
            conversation: conversation.to_string(),
            cancel: cancel.clone(),
            turn: None,
        });
        Slot {
            inner: Arc::clone(&self.inner),
            id,
        }
    }

    /// Records that call `id` answers the user turn stored at `turn`.
    fn claim(&self, id: u64, turn: String) {
        if let Some(call) = lock(&self.inner.running).iter_mut().find(|r| r.id == id) {
            call.turn = Some(turn);
        }
    }

    /// Whether a call that goes answers the user turn of `conversation` stored at `turn`.
    fn answering(&self, conversation: &str, turn: &str) -> bool {
        lock(&self.inner.running)
            .iter()
            .any(|r| r.conversation == conversation && r.turn.as_deref() == Some(turn))
    }

    fn read(&self, conversation: &str) -> io::Result<Vec<Record>> {
        lock(&self.inner.sink).read_session(SOURCE, conversation)
    }

    /// Appends one record and makes it durable (the session only, never the readers' progress).
    /// The record is stored after every record of its conversation that is stored already, and
    /// its time is later than theirs, whichever call stored them. Answers the time it got.
    fn store(&self, record: &Record) -> io::Result<String> {
        let mut clock = lock(&self.inner.clock);
        let mut record = record.clone();
        let session = record.session().to_owned();
        let seen = clock.get(&session).copied();
        let mut at = millis(record.time());
        if let Some(seen) = seen
            && at.is_none_or(|at| at <= seen)
        {
            at = Some(seen);
            let time = next_time(&mut at);
            set_time(&mut record, time);
        }
        if let Some(at) = millis(record.time()) {
            clock.insert(session.clone(), at);
        }
        let mut sink = lock(&self.inner.sink);
        sink.append(&record)?;
        sink.flush_session(SOURCE, &session)?;
        Ok(record.time().to_owned())
    }

    /// The answer: one request to the provider per model turn, and between two turns the tool calls
    /// of the first run. Every turn's text and every call and result are stored as they come.
    /// `emit` gets each piece of text and each call that starts, and returns `false` when the
    /// client is gone.
    async fn answer(
        &self,
        run: Run<'_>,
        conversation: &str,
        mut records: Vec<Record>,
        last: &mut Option<i64>,
        emit: &mut dyn FnMut(Piece<'_>) -> bool,
    ) -> End {
        let Run {
            target,
            cancel,
            usage,
            ..
        } = run;
        loop {
            let Ok(body) = request_body(&target.model, &records, self.inner.tools.listing()) else {
                return End::Failed(Failure::Internal);
            };
            usage.sent(body.len());
            let mut text = String::new();
            let mut calls = Calls::default();
            let end = self
                .stream_answer(target, body, cancel, &mut text, &mut calls, emit)
                .await;
            usage.received(text.len());
            if !text.is_empty() && self.store_text(conversation, last, text).is_err() {
                return End::Failed(Failure::CannotSave);
            }
            if end != End::Done {
                return end;
            }
            let calls = calls.finish();
            if calls.is_empty() {
                return End::Done;
            }
            // A call with no name cannot be answered or replayed: the turn is broken.
            if calls.iter().any(|call| call.name.is_empty()) {
                return End::Failed(Failure::Cut);
            }
            usage.calls(&calls);
            if let Err(end) = self.run_calls(&run, conversation, calls, last, emit).await {
                return end;
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
            tools: Vec::new(),
        }))
        .map(|_| ())
    }

    /// Store `calls`, then run them in order, storing each result. When the answer is stopped, every
    /// call that has no result yet gets [`STOPPED_RESULT`] and the error is `End::Stopped`. The
    /// client is told of each call as it starts, except the facts call, which the window never shows.
    async fn run_calls(
        &self,
        run: &Run<'_>,
        conversation: &str,
        calls: Vec<PendingCall>,
        last: &mut Option<i64>,
        emit: &mut dyn FnMut(Piece<'_>) -> bool,
    ) -> Result<(), End> {
        let cancel = run.cancel;
        let calls = tool_calls(calls, conversation);
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
                    result = self.run_call(run, call, emit) => Some(result),
                }
            } else {
                None
            };
            // A sub-agent that found the client gone has cancelled the answer: the call is stopped
            // even though it returned.
            let Some(result) = outcome.filter(|_| !cancel.is_cancelled()) else {
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
        .map(|_| ())
    }

    /// One call of the answer: the sub-agent tool, when the list has it, runs here in the chat;
    /// any other tool runs on a thread of its own.
    async fn run_call(
        &self,
        run: &Run<'_>,
        call: &ToolCall,
        emit: &mut dyn FnMut(Piece<'_>) -> bool,
    ) -> Result<String, String> {
        if call.name == subagent::NAME && self.inner.tools.find(subagent::NAME).is_some() {
            let nested = (subagent::INSTRUCTION, &self.inner.sub_tools);
            return self.run_subagent(run, &call.arguments, nested, emit).await;
        }
        self.run_tool(run.tools, &self.inner.tools, &call.name, &call.arguments)
            .await
    }

    /// One tool call of `tools`, on a thread of its own so that a stop does not wait for it.
    pub(crate) async fn run_tool(
        &self,
        count: &ToolCount,
        tools: &ChatTools,
        name: &str,
        arguments: &str,
    ) -> Result<String, String> {
        let Some(tool) = tools.find(name) else {
            return Err(format!("unknown tool {name}"));
        };
        let arguments = arguments.to_string();
        let running = InFlight::start(count);
        self.inner
            .handle
            .spawn_blocking(move || {
                let _running = running;
                tool.run(&arguments)
            })
            .await
            .unwrap_or_else(|_| Err("the tool failed".into()))
    }

    /// One request to the provider and its answer, until it ends. The text so far is in `text`
    /// whatever the end, and the tool calls the model asked for in `calls`. `emit` gets each piece
    /// of text and returns `false` when the client is gone.
    ///
    /// A model the API refuses function tools for next to a reasoning effort gets the request
    /// again with `reasoning_effort: "none"`, and every later request of its own with it.
    pub(crate) async fn stream_answer(
        &self,
        target: &Target,
        body: String,
        cancel: &CancellationToken,
        text: &mut String,
        calls: &mut Calls,
        emit: &mut dyn FnMut(Piece<'_>) -> bool,
    ) -> End {
        let body = target
            .prepared(body.as_bytes())
            .and_then(|plain| String::from_utf8(plain).ok())
            .unwrap_or(body);
        let end = self
            .stream_once(target, body.clone(), cancel, text, calls, emit)
            .await;
        if end != End::Failed(Failure::ToolsWithReasoning) {
            return end;
        }
        let Some(plain) =
            without_reasoning(body.as_bytes()).and_then(|plain| String::from_utf8(plain).ok())
        else {
            return end;
        };
        target.refused_tools_with_reasoning();
        self.stream_once(target, plain, cancel, text, calls, emit)
            .await
    }

    /// One request to the provider and its answer, as [`Chat::stream_answer`] without the retry.
    async fn stream_once(
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
        // A success that is no event stream is not an answer.
        if !is_event_stream(&response) {
            return End::Failed(Failure::Status(status.as_u16()));
        }
        let mut events = Events::new(response, self.inner.wire_limit);

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
                Ok(Some(Ok(data))) => data,
                // The wire cap is a cut, whatever the text holds.
                Ok(Some(Err(StreamError::Cap))) => return End::Failed(Failure::Cut),
                Ok(Some(Err(StreamError::Broken)) | None) => {
                    return broken(Failure::Unreachable);
                }
            };
            got_chunk = true;
            if event.trim() == "[DONE]" {
                return End::Done;
            }
            // An event that is no chunk (a keep-alive, a vendor's note) says nothing about the
            // answer: skip it.
            let Ok(chunk) = serde_json::from_str::<Chunk>(&event) else {
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

    /// Stop every call of `conversation`, and no call of another. `true` when one was running.
    #[must_use]
    pub fn stop(&self, conversation: &str) -> bool {
        let mut any = false;
        for call in lock(&self.inner.running)
            .iter()
            .filter(|r| r.conversation == conversation)
        {
            call.cancel.cancel();
            any = true;
        }
        any
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

    /// Every stored conversation with its records, the newest first, for a reader that copies them.
    ///
    /// # Errors
    /// When the conversations cannot be listed or one cannot be read.
    pub fn conversations(&self) -> io::Result<Vec<(String, Vec<Record>)>> {
        let sessions = lock(&self.inner.sink).list_sessions(SOURCE)?;
        sessions
            .into_iter()
            .map(|s| self.read(&s.session).map(|records| (s.session, records)))
            .collect()
    }

    /// The one line the chat shows for a call to the tool `name`.
    #[must_use]
    pub fn tool_line(&self, name: &str, arguments: &str) -> String {
        self.inner.tools.line(name, arguments)
    }

    /// Erases every conversation, for the person's erasure: every running answer is stopped first and
    /// waited for, so that nothing is written after the files are gone, and no answer can start
    /// until they are.
    ///
    /// # Errors
    /// The plain reason the conversations could not be removed.
    pub fn erase_all(&self) -> Result<(), String> {
        let deadline = std::time::Instant::now() + ERASE_WAIT;
        loop {
            let running = lock(&self.inner.running);
            if running.is_empty() {
                // The guard is held to the end: no answer starts while the files go.
                lock(&self.inner.clock).clear();
                return lock(&self.inner.sink)
                    .delete_source(SOURCE)
                    .map_err(|_| "the conversations could not be removed".to_owned());
            }
            for call in running.iter() {
                call.cancel.cancel();
            }
            drop(running);
            if std::time::Instant::now() >= deadline {
                return Err("the running answers did not stop".to_owned());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The JSON line for `chat-delete` of one conversation or `all`. A conversation that is
    /// answering is not deleted.
    #[must_use]
    pub fn delete(&self, which: &str) -> String {
        // The guard is held on purpose, to the end: no answer can start for the conversation
        // while it is deleted.
        let running = lock(&self.inner.running);
        if running
            .iter()
            .any(|r| which == "all" || r.conversation == which)
        {
            return error_response("busy");
        }
        let result = {
            let mut sink = lock(&self.inner.sink);
            if which == "all" {
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
            }
        };
        match result {
            Ok(n) => {
                // The running guard is still held, so no call stores into what went. Only the
                // deleted conversations lose their floor: that of any other is still in use.
                let mut clock = lock(&self.inner.clock);
                if which == "all" {
                    clock.clear();
                } else {
                    clock.remove(which);
                }
                json!({ "deleted": n }).to_string()
            }
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
            } else if refuses_tools_with_reasoning(status.as_u16(), body.as_bytes()) {
                Failure::ToolsWithReasoning
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

/// Puts `time` on `record`.
fn set_time(record: &mut Record, time: String) {
    match record {
        Record::Turn(r) => r.time = time,
        Record::ToolCall(r) => r.time = time,
        Record::ToolResult(r) => r.time = time,
        Record::Mark(_) => {}
    }
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
