//! Sends what the user said to the Construct host, so the profile is built from it: every user
//! prompt a record store takes in, from the log readers or from the chat, goes to the host's `manage-facts` route over its loopback port,
//! with the turns before it as context. The route plans with the model inside the request, so
//! prompts go one at a time from a queue on a thread of their own, and the readers never wait.
//!
//! A prompt typed after the agent's first start is *live* and goes out by itself. An older one is
//! history and goes out only when the user runs `backfill`, because every prompt costs a model
//! call. A live prompt goes before the rest of a backfill. It waits for the call in progress, and
//! after a failed or refused prompt for the pause that follows (`Timing`).
//!
//! The journal (see [`journal`]) keeps what was sent, so a restart, a crash or a second
//! `backfill` never sends a prompt twice. A prompt the host may have processed counts as sent
//! (`status` counts it as `unsure`). Prompt text is never logged, journaled or put in `status`.
//!
//! [`input`] shapes a call, [`call`] makes it and reads the answer, and [`journal`] keeps what was
//! sent. This module holds what they share: the queue, the thread, `backfill` and the counters.
//!
//! Every prompt is checked against [`Settings::collects`] before it is queued and again before it
//! is sent, so with collection off nothing is sent. A prompt typed while collection is off is
//! never sent: it is marked as refused in the journal, and `status` counts it as `refused`. A
//! prompt that collection refuses later, one queued when it went off or one from history, is not
//! marked: a live one is queued again at the next start, a history one goes out in a backfill, and
//! a backfill that is running stops.

mod call;
mod input;
mod journal;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use connectors::{Registry, Role, Tool, Turn, compare_times};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::chat::Link;
use crate::host::Connection;
use crate::ipc::{create_private_dir, error_response};
use crate::model::Target;
use crate::records::{FileSink, Sink, fnv};
use crate::settings::Settings;
use crate::sync::lock;
use call::{Answer, Reply, call};
use input::input;
use journal::{Journal, days_ago, first_start};

pub const CONTEXT_TURNS: usize = 10;
/// Days of history `backfill` looks at when none is given.
const DEFAULT_DAYS: u32 = 30;
/// Calls on one prompt before it is given up for now.
const ATTEMPTS: usize = 3;
/// Rounds of failed calls on a live prompt before it is left to the next start.
const MAX_STRIKES: u32 = 3;
/// Prompts of a backfill that fail one after the other before the run stops.
const MAX_FAILED_IN_A_ROW: u32 = 3;

/// How long the thread waits and pauses; the tests make it short.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// Pauses between the calls on one prompt.
    pub retry: [Duration; ATTEMPTS - 1],
    /// How often the thread looks for a host or a model while it has none.
    pub idle: Duration,
    /// The pause after a prompt failed, for every prompt that follows. A new prompt does not
    /// cut it short.
    pub failed: Duration,
    /// The pause after the host refused the agent.
    pub refused: Duration,
    /// How long one call may take; the route's own limit is shorter.
    pub call: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            retry: [Duration::from_secs(2), Duration::from_secs(10)],
            idle: Duration::from_secs(2),
            failed: Duration::from_secs(60),
            refused: Duration::from_secs(300),
            call: Duration::from_secs(45),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Prompt {
    tool: String,
    session: String,
    time: String,
    text: String,
}

impl Prompt {
    fn of(turn: &Turn) -> Self {
        Self {
            tool: turn.tool.clone(),
            session: turn.session.clone(),
            time: turn.time.clone(),
            text: turn.text.clone(),
        }
    }

    fn key(&self) -> u64 {
        fnv(&[&self.tool, &self.session, &self.time, &self.text])
    }
}

/// Which queue a prompt comes from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Live,
    Backfill,
}

#[derive(Default)]
struct Queue {
    live: VecDeque<Prompt>,
    /// Keys of the live prompts in `live`, so a prompt is never queued twice.
    queued: HashSet<u64>,
    /// The prompts of the backfill that runs, oldest first.
    backfill: Option<VecDeque<Prompt>>,
}

#[derive(Default)]
struct Counts {
    live_sent: u64,
    live_stored: u64,
    live_failed: u64,
    /// Prompts the host may have processed and the agent does not know.
    unsure: u64,
    added: u64,
    error: Option<String>,
    backfill: BackfillCounts,
}

#[derive(Default)]
struct BackfillCounts {
    /// `idle` until a run started, then `running`, `done` or `stopped`.
    state: &'static str,
    total: u64,
    sent: u64,
    stored: u64,
    failed: u64,
    error: Option<String>,
}

/// The arguments of `backfill`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct BackfillRequest {
    days: u32,
    limit: Option<usize>,
    dry_run: bool,
}

impl Default for BackfillRequest {
    fn default() -> Self {
        Self {
            days: DEFAULT_DAYS,
            limit: None,
            dry_run: false,
        }
    }
}

struct Inner {
    data_dir: PathBuf,
    first_start: String,
    tools: Vec<&'static str>,
    settings: Settings,
    link: Link,
    timing: Timing,
    journal: Mutex<Journal>,
    queue: Mutex<Queue>,
    wake: Condvar,
    counts: Mutex<Counts>,
}

/// The prompts the user typed, sent to the host. Cheap to clone.
#[derive(Clone)]
pub struct Manager {
    inner: Arc<Inner>,
}

impl Manager {
    /// The manager over the files in `data_dir`. The first start is noted here, if it is the first.
    ///
    /// # Errors
    /// Fails when the directory or its files cannot be created or read.
    pub fn open(data_dir: &Path, settings: Settings, link: Link) -> io::Result<Self> {
        Self::open_with(data_dir, settings, link, Timing::default())
    }

    /// Like [`Manager::open`], with these pauses; for the tests.
    ///
    /// # Errors
    /// Fails when the directory or its files cannot be created or read.
    pub fn open_with(
        data_dir: &Path,
        settings: Settings,
        link: Link,
        timing: Timing,
    ) -> io::Result<Self> {
        create_private_dir(data_dir)?;
        let mut tools: Vec<&'static str> = Registry::builtin().iter().map(Tool::name).collect();
        tools.push(crate::chat::SOURCE);
        Ok(Self {
            inner: Arc::new(Inner {
                data_dir: data_dir.to_owned(),
                first_start: first_start(data_dir)?,
                tools,
                settings,
                link,
                timing,
                journal: Mutex::new(Journal::open(data_dir)?),
                queue: Mutex::new(Queue::default()),
                wake: Condvar::new(),
                counts: Mutex::new(Counts::default()),
            }),
        })
    }

    /// What a record store calls with every new user turn (the log readers' and the chat's): a
    /// prompt from after the first start is queued. It never blocks.
    #[must_use]
    pub fn observer(&self) -> crate::follow::Observer {
        let inner = Arc::clone(&self.inner);
        // The stores call it for user turns only.
        Arc::new(move |turn: &Turn| inner.offer(Prompt::of(turn)))
    }

    /// Starts the thread that sends the queued prompts.
    pub fn spawn(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::spawn(move || inner.run());
    }

    /// The `backfill` command: `argument` is a JSON object with `days`, `limit` and `dry_run`.
    /// The answer is one JSON line.
    #[must_use]
    pub fn backfill(&self, argument: &str) -> String {
        let request = if argument.trim().is_empty() {
            Ok(BackfillRequest::default())
        } else {
            serde_json::from_str::<BackfillRequest>(argument)
        };
        let Ok(request) = request else {
            return error_response("the arguments of backfill are not valid");
        };
        if request.days == 0 || request.limit == Some(0) {
            return error_response("days and limit must be positive");
        }
        match self.inner.backfill(&request) {
            Ok(reply) => reply.to_string(),
            Err(message) => error_response(&message),
        }
    }

    /// What `status` shows as `facts`: counts and states, never a prompt.
    #[must_use]
    pub fn status(&self) -> Value {
        self.inner.status()
    }
}

/// Why a prompt did not go out.
enum Failure {
    /// The calls did not get an answer worth keeping; a later call may.
    Unavailable(String),
    /// The host refuses every call of the agent.
    Refused(String),
    /// The host refuses this prompt, and would refuse it again.
    Rejected(String),
}

#[derive(Clone, Copy)]
enum Done {
    Stored {
        added: u64,
    },
    Nothing,
    /// The host may have processed the prompt: it is not sent again.
    Unsure,
    /// Not sent: it was sent before, or holds no text.
    Skipped,
}

impl Inner {
    /// A prompt that was just stored; see the module doc for what is refused.
    fn offer(&self, prompt: Prompt) {
        if !self.is_live(&prompt) {
            return;
        }
        if !self.settings.collects(&prompt.tool) {
            if let Err(e) = lock(&self.journal).refuse(prompt.key()) {
                self.note_error(&format!("cannot write the journal: {e}"));
            }
            return;
        }
        self.enqueue(prompt);
    }

    /// Whether the prompt was typed after the agent's first start.
    fn is_live(&self, prompt: &Prompt) -> bool {
        compare_times(&prompt.time, &self.first_start).is_ge()
    }

    /// Queues a live prompt.
    fn enqueue(&self, prompt: Prompt) {
        let key = prompt.key();
        if lock(&self.journal).is_done(key) {
            return;
        }
        let mut queue = lock(&self.queue);
        if queue.queued.insert(key) {
            queue.live.push_back(prompt);
            drop(queue);
            self.wake.notify_all();
        }
    }

    /// The user prompts of every tool, oldest first. It reads every record file.
    fn prompts(&self) -> io::Result<Vec<Prompt>> {
        let sink = FileSink::new(&self.data_dir);
        let mut all = Vec::new();
        for tool in &self.tools {
            all.extend(sink.user_turns(tool)?.iter().map(Prompt::of));
        }
        all.sort_by(|a, b| compare_times(&a.time, &b.time));
        Ok(all)
    }

    /// Queues the live prompts that were stored but not sent before the agent stopped.
    fn recover(&self) {
        match self.prompts() {
            Ok(all) => all
                .into_iter()
                .filter(|p| self.is_live(p) && self.settings.collects(&p.tool))
                .for_each(|p| self.enqueue(p)),
            Err(e) => self.note_error(&format!("cannot read the records: {e}")),
        }
    }

    fn model_is_set(&self) -> bool {
        Target::from_settings(&self.settings.model()).is_some()
    }

    fn backfill(&self, request: &BackfillRequest) -> Result<Value, String> {
        let since = days_ago(request.days).map_err(|e| e.to_string())?;
        // The files are read before the journal is locked: the readers and `status` use it.
        let all = self
            .prompts()
            .map_err(|e| format!("cannot read the records: {e}"))?;
        let candidates: Vec<Prompt> = {
            let journal = lock(&self.journal);
            all.into_iter()
                .filter(|p| {
                    self.settings.collects(&p.tool)
                        && compare_times(&p.time, &since).is_ge()
                        && compare_times(&p.time, &self.first_start).is_lt()
                        && !journal.is_done(p.key())
                })
                .collect()
        };
        let mut per_tool: BTreeMap<&str, usize> =
            self.tools.iter().map(|tool| (*tool, 0)).collect();
        for p in &candidates {
            if let Some(n) = per_tool.get_mut(p.tool.as_str()) {
                *n += 1;
            }
        }
        let total = candidates.len();
        let chosen = request.limit.map_or(total, |limit| limit.min(total));
        if request.dry_run {
            return Ok(json!({
                "dry_run": true,
                "days": request.days,
                "prompts": per_tool,
                "total": total,
                "would_send": chosen,
            }));
        }
        if self.link.get().is_none() {
            return Err("Construct is not running yet".into());
        }
        if !self.model_is_set() {
            return Err("no model is set: save its address, name and key in the settings".into());
        }
        let mut queue = lock(&self.queue);
        if queue.backfill.as_ref().is_some_and(|left| !left.is_empty()) {
            return Err("a backfill is running; see `status`".into());
        }
        queue.backfill = Some(candidates.into_iter().take(chosen).collect());
        drop(queue);
        lock(&self.counts).backfill = BackfillCounts {
            state: if chosen == 0 { "done" } else { "running" },
            total: chosen as u64,
            ..BackfillCounts::default()
        };
        self.wake.notify_all();
        Ok(json!({
            "started": chosen > 0,
            "days": request.days,
            "prompts": per_tool,
            "queued": chosen,
        }))
    }

    fn status(&self) -> Value {
        let (queued, backfill_left) = {
            let queue = lock(&self.queue);
            (
                queue.live.len(),
                queue.backfill.as_ref().map_or(0, VecDeque::len),
            )
        };
        let (sent, refused) = {
            let journal = lock(&self.journal);
            (journal.sent.len(), journal.refused.len())
        };
        let counts = lock(&self.counts);
        let b = &counts.backfill;
        let mut backfill = json!({
            "state": if b.state.is_empty() { "idle" } else { b.state },
            "total": b.total,
            "sent": b.sent,
            "stored": b.stored,
            "failed": b.failed,
            "left": backfill_left,
        });
        if let Some(error) = &b.error {
            backfill["error"] = error.clone().into();
        }
        let mut facts = json!({
            "since": self.first_start,
            "sent": sent,
            "added": counts.added,
            "unsure": counts.unsure,
            "refused": refused,
            "live": {
                "queued": queued,
                "sent": counts.live_sent,
                "stored": counts.live_stored,
                "failed": counts.live_failed,
            },
            "backfill": backfill,
        });
        if let Some(error) = &counts.error {
            facts["error"] = error.clone().into();
        }
        facts
    }

    fn note_error(&self, message: &str) {
        lock(&self.counts).error = Some(message.to_owned());
    }

    /// Waits for a new prompt or a backfill, at most `idle`.
    fn wait_for_work(&self) {
        let queue = lock(&self.queue);
        let _ = self.wake.wait_timeout(queue, self.timing.idle);
    }

    /// The prompt to send next, live ones first. It stays in its queue until it is settled.
    fn next(&self) -> Option<(Source, Prompt)> {
        let queue = lock(&self.queue);
        if let Some(p) = queue.live.front() {
            return Some((Source::Live, p.clone()));
        }
        queue
            .backfill
            .as_ref()
            .and_then(VecDeque::front)
            .map(|p| (Source::Backfill, p.clone()))
    }

    fn remove(&self, source: Source, key: u64) {
        let mut queue = lock(&self.queue);
        match source {
            Source::Live => {
                queue.live.pop_front();
                queue.queued.remove(&key);
            }
            Source::Backfill => {
                if let Some(left) = queue.backfill.as_mut() {
                    left.pop_front();
                }
            }
        }
    }

    fn run(&self) {
        self.recover();
        // Failed rounds of each live prompt in the queue.
        let mut strikes: HashMap<u64, u32> = HashMap::new();
        let mut failed_in_a_row = 0_u32;
        loop {
            let Some(connection) = self.link.get() else {
                self.wait_for_work();
                continue;
            };
            if !self.model_is_set() {
                self.wait_for_work();
                continue;
            }
            let Some((source, prompt)) = self.next() else {
                self.wait_for_work();
                continue;
            };
            let key = prompt.key();
            if !self.settings.collects(&prompt.tool) {
                // Collection went off while it waited; see the module doc.
                self.remove(source, key);
                if source == Source::Backfill {
                    self.stop_backfill("collection is off");
                }
                continue;
            }
            match self.send(&connection, &prompt) {
                Ok(done) => {
                    strikes.remove(&key);
                    failed_in_a_row = 0;
                    self.remove(source, key);
                    self.settled(source, done);
                }
                Err(Failure::Unavailable(reason)) => {
                    self.note_error(&reason);
                    match source {
                        Source::Live => {
                            let strike = strikes.entry(key).or_default();
                            *strike += 1;
                            if *strike >= MAX_STRIKES {
                                // Left for the next start, which queues it again.
                                strikes.remove(&key);
                                self.remove(source, key);
                                lock(&self.counts).live_failed += 1;
                            }
                        }
                        Source::Backfill => {
                            // The rest of a backfill would fail the same way: stop it after a few.
                            self.remove(source, key);
                            failed_in_a_row += 1;
                            lock(&self.counts).backfill.failed += 1;
                            if failed_in_a_row >= MAX_FAILED_IN_A_ROW {
                                self.stop_backfill(&reason);
                                failed_in_a_row = 0;
                            }
                        }
                    }
                    // A pause after a failure is for the host to recover: nothing cuts it short.
                    std::thread::sleep(self.timing.failed);
                }
                Err(Failure::Refused(reason)) => {
                    self.note_error(&reason);
                    if source == Source::Backfill {
                        self.stop_backfill(&reason);
                    }
                    std::thread::sleep(self.timing.refused);
                }
                Err(Failure::Rejected(reason)) => {
                    // The mark stays: the host refused this prompt as such. A host that refuses
                    // every prompt (a changed schema) is stopped after a few, not drained.
                    self.note_error(&reason);
                    self.remove(source, key);
                    failed_in_a_row += 1;
                    {
                        let mut counts = lock(&self.counts);
                        match source {
                            Source::Live => counts.live_failed += 1,
                            Source::Backfill => counts.backfill.failed += 1,
                        }
                        self.finish_backfill(source, &mut counts);
                    }
                    if failed_in_a_row >= MAX_FAILED_IN_A_ROW {
                        if source == Source::Backfill {
                            self.stop_backfill(&reason);
                        }
                        failed_in_a_row = 0;
                        std::thread::sleep(self.timing.failed);
                    }
                }
            }
        }
    }

    fn stop_backfill(&self, reason: &str) {
        lock(&self.queue).backfill = None;
        let mut counts = lock(&self.counts);
        counts.backfill.state = "stopped";
        counts.backfill.error = Some(reason.to_owned());
    }

    fn settled(&self, source: Source, done: Done) {
        let mut counts = lock(&self.counts);
        let (stored, added) = match done {
            Done::Stored { added } => (true, added),
            Done::Nothing => (false, 0),
            Done::Unsure => {
                counts.unsure += 1;
                (false, 0)
            }
            Done::Skipped => {
                self.finish_backfill(source, &mut counts);
                return;
            }
        };
        counts.added += added;
        match source {
            Source::Live => {
                counts.live_sent += 1;
                counts.live_stored += u64::from(stored);
            }
            Source::Backfill => {
                counts.backfill.sent += 1;
                counts.backfill.stored += u64::from(stored);
            }
        }
        self.finish_backfill(source, &mut counts);
        counts.error = None;
    }

    /// Marks a backfill done when its last prompt is out of the queue.
    fn finish_backfill(&self, source: Source, counts: &mut Counts) {
        if source != Source::Backfill {
            return;
        }
        let finished = lock(&self.queue)
            .backfill
            .as_ref()
            .is_none_or(VecDeque::is_empty);
        if finished && counts.backfill.state == "running" {
            counts.backfill.state = "done";
        }
    }

    /// The context without what was typed while collection was off: those prompts and the answers
    /// to them stay on the machine, like the prompts themselves.
    fn without_refused(&self, turns: Vec<Turn>) -> Vec<Turn> {
        let journal = lock(&self.journal);
        let mut skipping = false;
        turns
            .into_iter()
            .filter(|t| {
                if t.role == Role::User {
                    skipping = journal.refused.contains(&Prompt::of(t).key());
                }
                !skipping
            })
            .collect()
    }

    /// One prompt: its context is read, it is marked in the journal, and then up to [`ATTEMPTS`]
    /// calls are made.
    fn send(&self, connection: &Connection, prompt: &Prompt) -> Result<Done, Failure> {
        if prompt.text.trim().is_empty() {
            return Ok(Done::Skipped);
        }
        // A chat conversation that was deleted while its message waited: nothing to send.
        if prompt.tool == crate::chat::SOURCE
            && FileSink::new(&self.data_dir)
                .read_session(&prompt.tool, &prompt.session)
                .map_err(|e| Failure::Unavailable(format!("cannot read the records: {e}")))?
                .is_empty()
        {
            return Ok(Done::Skipped);
        }
        // Before the mark: a prompt whose context cannot be read is not sent without it.
        // The whole session is read and cleaned before the last turns are taken, so a window never
        // opens in the middle of an answer to a prompt that was refused.
        let mut before = FileSink::new(&self.data_dir)
            .turns_before(&prompt.tool, &prompt.session, &prompt.time, usize::MAX)
            .map_err(|e| Failure::Unavailable(format!("cannot read the records: {e}")))?;
        before = self.without_refused(before);
        before.drain(..before.len().saturating_sub(CONTEXT_TURNS));
        let key = prompt.key();
        match lock(&self.journal).begin(key) {
            Ok(true) => {}
            // Sent already, by the other queue or before a restart.
            Ok(false) => return Ok(Done::Skipped),
            Err(e) => {
                return Err(Failure::Unavailable(format!(
                    "cannot write the journal: {e}"
                )));
            }
        }
        let (message, conversation) = input(&prompt.text, &before);
        let body = json!({ "message": message, "conversation": conversation }).to_string();
        let mut last = String::new();
        for attempt in 0..ATTEMPTS {
            match call(connection, &body, self.timing.call) {
                Reply::Answered(Answer::Stored { added }) => return Ok(Done::Stored { added }),
                Reply::Answered(Answer::Nothing) => return Ok(Done::Nothing),
                // A call that may have run is not sent again.
                Reply::MaybeProcessed => return Ok(Done::Unsure),
                Reply::HostRefused(reason) => {
                    self.take_back(key);
                    return Err(Failure::Refused(reason));
                }
                Reply::PromptRefused(reason) => return Err(Failure::Rejected(reason)),
                Reply::Answered(Answer::Failed(reason)) | Reply::NotDelivered(reason) => {
                    last = reason;
                }
            }
            if let Some(pause) = self.timing.retry.get(attempt) {
                std::thread::sleep(*pause);
            }
        }
        self.take_back(key);
        Err(Failure::Unavailable(last))
    }

    fn take_back(&self, key: u64) {
        if let Err(e) = lock(&self.journal).undo(key) {
            self.note_error(&format!("cannot write the journal: {e}"));
        }
    }
}
