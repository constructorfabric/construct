//! Keeps the activity store up to date while the follower writes records.
//!
//! The follower pushes every record it stores to a [`Feed`], a queue that never blocks: the
//! read loop only takes a short lock. A task of its own ([`Feed::run`]) builds the store from
//! the record files first, in the background, and then applies the queued records one by one.
//! A write that fails is counted and tried again; a record that keeps failing, and a queue that
//! overflowed, are made good by building the store again from the files, which stay the single
//! source.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use connectors::Record;
use tokio::sync::Notify;

use super::Store;
use crate::records::{FileSink, Recorder};
use crate::sync::lock;

/// Records the queue holds, and the bytes of text they carry (a turn is queued whole, for the
/// store keys it by a hash of its text). A push past either limit is dropped, and the next turn
/// of the task builds the store again from the files, which hold what the queue lost.
const MAX_QUEUED: usize = 100_000;
/// The most turn text the queue holds.
const MAX_QUEUED_BYTES: usize = 64 * 1024 * 1024;
/// The pause after the next failed build: the first one after a build that worked, then twice
/// the last, up to [`MAX_REBUILD_RETRY`].
fn next_rebuild_pause(last: Duration, built: bool) -> Duration {
    if built {
        REBUILD_RETRY
    } else {
        (last * 2).min(MAX_REBUILD_RETRY)
    }
}

/// What a queued record costs besides its text, roughly.
const RECORD_OVERHEAD: usize = 256;
/// Tries a record gets before the store is built again instead. One writer talks to a local
/// socket, so the pauses need no jitter; a longer outage is the build's to ride out.
const MAX_ATTEMPTS: u32 = 5;
/// Pause after a failed write; it doubles on every try.
const FIRST_RETRY: Duration = Duration::from_millis(500);
/// Pause after a failed build; it doubles on every failure, up to [`MAX_REBUILD_RETRY`], so a
/// record the database always refuses does not make the agent read all history every 30 s.
const REBUILD_RETRY: Duration = Duration::from_secs(30);
/// The longest pause between two builds.
const MAX_REBUILD_RETRY: Duration = Duration::from_mins(10);

/// Where the store is, as `status` tells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// The database is not up yet.
    Waiting,
    /// The store is being built from the record files.
    ReadingHistory,
    /// Records are applied as they arrive.
    Ready,
    /// The store is behind and the last build failed; the task tries again.
    Behind,
    /// The store cannot be opened; the agent runs without it.
    Off,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::ReadingHistory => "reading_history",
            Self::Ready => "ready",
            Self::Behind => "behind",
            Self::Off => "off",
        }
    }
}

/// What a queued record costs: its text and a fixed share for the rest.
fn weight(record: &Record) -> usize {
    RECORD_OVERHEAD
        + match record {
            Record::Turn(turn) => turn.text.len(),
            _ => 0,
        }
}

struct Queue {
    records: VecDeque<Record>,
    /// Bytes the queued records weigh, as [`weight`] counts them.
    bytes: usize,
    /// The store may miss records: the queue overflowed or a record could not be applied.
    stale: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    wake: Notify,
    state: Mutex<(Phase, Option<String>)>,
    /// Records of the current build that were read.
    history_done: AtomicU64,
    /// Writes that failed, since the agent started.
    failed_writes: AtomicU64,
}

/// The queue between the follower and the activity store, and what `status` shows of it.
#[derive(Clone)]
pub struct Feed(Arc<Shared>);

impl Default for Feed {
    fn default() -> Self {
        Self::new()
    }
}

impl Feed {
    /// An empty feed. The store is built from the files first.
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(Shared {
            queue: Mutex::new(Queue {
                records: VecDeque::new(),
                bytes: 0,
                stale: true,
            }),
            wake: Notify::new(),
            state: Mutex::new((Phase::Waiting, None)),
            history_done: AtomicU64::new(0),
            failed_writes: AtomicU64::new(0),
        }))
    }

    /// What the sinks that write record files call with each new record.
    #[must_use]
    pub fn recorder(&self) -> Recorder {
        let feed = self.clone();
        Arc::new(move |record| feed.push(record))
    }

    /// Queues a record that is on the disk. It never waits for the store. A record the store
    /// ignores is not queued, and none is while nothing drains the queue: before the first build,
    /// while a failed build waits to be tried again, and when the store cannot be opened (the
    /// build reads the files).
    pub fn push(&self, record: &Record) {
        if !super::is_activity(record)
            || matches!(
                lock(&self.0.state).0,
                Phase::Waiting | Phase::Behind | Phase::Off
            )
        {
            return;
        }
        {
            let mut queue = lock(&self.0.queue);
            let bytes = weight(record);
            if queue.records.len() >= MAX_QUEUED || queue.bytes + bytes > MAX_QUEUED_BYTES {
                queue.stale = true;
            } else {
                queue.bytes += bytes;
                queue.records.push_back(record.clone());
            }
        }
        self.0.wake.notify_one();
    }

    /// Says the store cannot be opened, with the reason.
    pub fn off(&self, reason: &str) {
        self.set(Phase::Off, Some(reason.to_owned()));
    }

    fn log(message: &str) {
        eprintln!("activity store: {message}");
    }

    /// The `activity` object of the `status` answer.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let (phase, error) = lock(&self.0.state).clone();
        let mut value = serde_json::json!({ "state": phase.as_str() });
        if phase == Phase::ReadingHistory {
            value["records_done"] = self.0.history_done.load(Ordering::Relaxed).into();
        }
        let failed = self.0.failed_writes.load(Ordering::Relaxed);
        if failed > 0 {
            value["failed_writes"] = failed.into();
        }
        if let Some(error) = error {
            value["error"] = error.into();
        }
        value
    }

    fn set(&self, phase: Phase, error: Option<String>) {
        *lock(&self.0.state) = (phase, error);
    }

    fn set_error(&self, error: Option<String>) {
        lock(&self.0.state).1 = error;
    }

    /// The next queued record; when the store may miss some, a build is due instead and the
    /// queue is emptied, for the files hold all that was queued before it.
    fn next(&self) -> Next {
        let mut queue = lock(&self.0.queue);
        if queue.stale {
            queue.stale = false;
            queue.records.clear();
            queue.bytes = 0;
            return Next::Rebuild;
        }
        match queue.records.pop_front() {
            Some(record) => {
                queue.bytes -= weight(&record);
                Next::Record(record)
            }
            None => Next::Empty,
        }
    }

    fn mark_stale(&self) {
        lock(&self.0.queue).stale = true;
    }

    /// Builds the store from the files, with the count of records read shown in `status`.
    async fn rebuild(&self, store: &Store, files: &FileSink, retry: Duration) -> bool {
        self.0.history_done.store(0, Ordering::Relaxed);
        self.set(Phase::ReadingHistory, None);
        Self::log("building from the record files");
        match store.rebuild_counting(files, &self.0.history_done).await {
            Ok(applied) => {
                Self::log(&format!("ready: {applied} records applied"));
                self.set(Phase::Ready, None);
                true
            }
            Err(e) => {
                Self::log(&format!(
                    "build failed, trying again in {} s: {e}",
                    retry.as_secs()
                ));
                self.0.failed_writes.fetch_add(1, Ordering::Relaxed);
                self.set(Phase::Behind, Some(e.to_string()));
                false
            }
        }
    }

    /// Applies one record, trying again after a pause; false when it keeps failing.
    async fn apply(&self, store: &Store, record: &Record) -> bool {
        let mut pause = FIRST_RETRY;
        for attempt in 1..=MAX_ATTEMPTS {
            match store.apply(record).await {
                Ok(()) => {
                    self.set_error(None);
                    return true;
                }
                Err(e) => {
                    self.0.failed_writes.fetch_add(1, Ordering::Relaxed);
                    self.set_error(Some(e.to_string()));
                    Self::log(&format!(
                        "write failed (try {attempt} of {MAX_ATTEMPTS}): {e}"
                    ));
                    if attempt < MAX_ATTEMPTS {
                        tokio::time::sleep(pause).await;
                        pause *= 2;
                    } else {
                        Self::log("giving up on a record, building again from the files");
                    }
                }
            }
        }
        false
    }

    /// Builds `store` from `files`, then applies every record pushed to the feed, for as long as
    /// the task runs. Nothing here ends the agent: a failure shows in `status`.
    pub async fn run(&self, store: &Store, files: &FileSink) {
        let mut retry = REBUILD_RETRY;
        loop {
            match self.next() {
                Next::Rebuild => {
                    let built = self.rebuild(store, files, retry).await;
                    if !built {
                        // The queue was dropped for the build, which reads the files; try again.
                        self.mark_stale();
                        tokio::time::sleep(retry).await;
                    }
                    retry = next_rebuild_pause(retry, built);
                }
                Next::Record(record) => {
                    if !self.apply(store, &record).await {
                        self.mark_stale();
                    }
                }
                Next::Empty => self.0.wake.notified().await,
            }
        }
    }
}

enum Next {
    Rebuild,
    Record(Record),
    Empty,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_tells_the_phase_and_the_failures() {
        let feed = Feed::new();
        assert_eq!(feed.to_json(), serde_json::json!({ "state": "waiting" }));
        feed.set(Phase::ReadingHistory, None);
        feed.0.history_done.store(42, Ordering::Relaxed);
        assert_eq!(
            feed.to_json(),
            serde_json::json!({ "state": "reading_history", "records_done": 42 })
        );
        feed.0.failed_writes.store(2, Ordering::Relaxed);
        feed.set(Phase::Ready, Some("refused".into()));
        assert_eq!(
            feed.to_json(),
            serde_json::json!({ "state": "ready", "failed_writes": 2, "error": "refused" })
        );
        feed.off("no database");
        assert_eq!(feed.to_json()["state"], "off");
    }

    #[test]
    fn a_queue_heavy_with_text_asks_for_a_build_by_bytes() {
        let feed = Feed::new();
        feed.set(Phase::Ready, None);
        let _ = feed.next();
        let turn = |text: String| {
            Record::Turn(connectors::Turn {
                tools: Vec::new(),
                tool: "t".into(),
                session: "s".into(),
                time: "2026-11-11T10:00:00Z".into(),
                folder: String::new(),
                branch: String::new(),
                role: connectors::Role::User,
                author: connectors::Author::Person,
                text,
            })
        };
        feed.push(&turn("x".repeat(MAX_QUEUED_BYTES / 2)));
        feed.push(&turn("y".repeat(MAX_QUEUED_BYTES / 2)));
        assert_eq!(lock(&feed.0.queue).records.len(), 1);
        assert!(matches!(feed.next(), Next::Rebuild));
        assert_eq!(lock(&feed.0.queue).bytes, 0);
    }

    #[test]
    fn the_rebuild_pause_doubles_up_to_the_cap_and_starts_again_after_a_build() {
        let mut pause = REBUILD_RETRY;
        let mut seen = vec![pause];
        for _ in 0..8 {
            pause = next_rebuild_pause(pause, false);
            seen.push(pause);
        }
        let secs: Vec<u64> = seen.iter().map(Duration::as_secs).collect();
        assert_eq!(secs, [30, 60, 120, 240, 480, 600, 600, 600, 600]);
        assert_eq!(next_rebuild_pause(pause, true), REBUILD_RETRY);
    }

    #[test]
    fn the_bytes_of_the_queue_go_back_to_nothing_as_records_are_taken() {
        let feed = Feed::new();
        feed.set(Phase::Ready, None);
        let _ = feed.next();
        let turn = Record::Turn(connectors::Turn {
            tools: Vec::new(),
            tool: "t".into(),
            session: "s".into(),
            time: "2026-11-11T10:00:00Z".into(),
            folder: String::new(),
            branch: String::new(),
            role: connectors::Role::User,
            author: connectors::Author::Person,
            text: "hello".into(),
        });
        feed.push(&turn);
        feed.push(&turn);
        assert_eq!(lock(&feed.0.queue).bytes, 2 * (RECORD_OVERHEAD + 5));
        assert!(matches!(feed.next(), Next::Record(_)));
        assert!(matches!(feed.next(), Next::Record(_)));
        assert_eq!(lock(&feed.0.queue).bytes, 0);
    }

    #[test]
    fn a_build_that_failed_shows_as_behind() {
        let feed = Feed::new();
        feed.set(Phase::Behind, Some("refused".into()));
        assert_eq!(
            feed.to_json(),
            serde_json::json!({ "state": "behind", "error": "refused" })
        );
    }

    #[test]
    fn a_full_queue_drops_the_push_and_asks_for_a_build() {
        let feed = Feed::new();
        feed.set(Phase::Ready, None);
        assert!(matches!(feed.next(), Next::Rebuild));
        let record = Record::Mark(connectors::Mark {
            tools: Vec::new(),
            tool: "t".into(),
            session: "s".into(),
            time: "2026-11-11T10:00:00Z".into(),
            folder: String::new(),
            branch: String::new(),
        });
        for _ in 0..=MAX_QUEUED {
            feed.push(&record);
        }
        assert_eq!(lock(&feed.0.queue).records.len(), MAX_QUEUED);
        assert_eq!(lock(&feed.0.queue).bytes, MAX_QUEUED * RECORD_OVERHEAD);
        assert!(matches!(feed.next(), Next::Rebuild));
        assert!(matches!(feed.next(), Next::Empty));
    }

    #[test]
    fn only_records_the_store_uses_are_queued_while_something_drains() {
        let feed = Feed::new();
        let mark = Record::Mark(connectors::Mark {
            tools: Vec::new(),
            tool: "t".into(),
            session: "s".into(),
            time: "2026-11-11T10:00:00Z".into(),
            folder: String::new(),
            branch: String::new(),
        });
        let call = Record::ToolCall(connectors::ToolCall {
            tool: "t".into(),
            session: "s".into(),
            time: "2026-11-11T10:00:00Z".into(),
            call_id: "c".into(),
            name: "n".into(),
            arguments: "{}".into(),
        });
        // Before the first build, and when the store is off, the files hold everything.
        feed.push(&mark);
        feed.off("no database");
        feed.push(&mark);
        assert!(lock(&feed.0.queue).records.is_empty());
        feed.set(Phase::ReadingHistory, None);
        feed.push(&call);
        feed.push(&Record::ToolResult(connectors::ToolResult {
            tool: "t".into(),
            session: "s".into(),
            time: "2026-11-11T10:00:00Z".into(),
            call_id: "c".into(),
            text: "{}".into(),
        }));
        assert!(lock(&feed.0.queue).records.is_empty());
        feed.push(&mark);
        assert_eq!(lock(&feed.0.queue).records.len(), 1);
    }
}
