//! The person's screen samples: what is in front on this computer, taken every
//! [`INTERVAL`] while the person has switched `collection.screen` on, and kept as the source
//! `screen` in the records folder.
//!
//! The signals come from [`Signals`]: the program of each platform gives one, and the tests give
//! a fake. A call to it is bounded in time ([`CALL_LIMIT`]): the call runs on a thread of its
//! own, and one that does not answer in time is given up, so the loop never waits for a stuck
//! call: the next step comes one interval after the last one ended. A few calls that never
//! return stop the sampler asking again (see [`MAX_STUCK`]) until one of them does; a call that
//! panics counts as returned.
//!
//! The samples are not session records: they have a type of their own and are kept in
//! `records/screen/<day>.jsonl` (the day is the local calendar day), user-only like the rest of
//! the data directory. A run of samples that show the same thing is one line, with the time of
//! its first and of its last sample and the number of samples in it; the idle time counts as
//! the same while it grows by as many seconds as the clock does. A run ends whenever a step
//! stores nothing (the switch went off, or the signals failed), so a line never spans a step
//! that stored nothing. The readers of session logs skip this folder.
//!
//! What a sample holds is the person's own data, kept as it is: the title and address of a
//! private window too, with the `private` flag set so that a reader can honour it. A title, an
//! address or an application name is never logged, put in an error or sent over the socket:
//! [`Reading`] hides them in `Debug`, and the `status` row has counts, times and fixed texts
//! only.

use std::fmt;
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use tokio::sync::Semaphore;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::erase::OwnStore;
use crate::follow::rfc3339;
use crate::records::{
    FileSink, SCREEN_SOURCE, ends_cleanly, private_dirs, private_options, read_lines, sync_chain,
};
use crate::settings::Settings;
use crate::sync::lock;

/// The source the samples belong to; also the name of the switch in the settings.
pub const SOURCE: &str = SCREEN_SOURCE;
/// Time between two samples.
pub const INTERVAL: Duration = Duration::from_secs(5);
/// The longest a call to the signals may take.
pub const CALL_LIMIT: Duration = Duration::from_secs(2);
/// Calls to [`Signals::read`] that did not return yet, at most. While this many are out, no new
/// read is made.
pub const MAX_STUCK: usize = 3;
/// While the switch is off, the grants are looked at once in this many steps.
const GRANTS_EVERY_WHEN_OFF: u32 = 12;
/// A run of one line is cut after this long, whatever the samples show.
const MAX_RUN: Duration = Duration::from_hours(1);
/// A run ends when the gap since its last sample is longer than this many intervals.
const MAX_GAP_INTERVALS: u32 = 3;
/// Longest gap, in seconds, between the idle time and the clock for two samples to be one run.
const IDLE_SLACK: u64 = 1;

/// The front application.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct App {
    pub name: String,
    pub bundle_id: String,
}

/// The open tab of a browser that is in front.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tab {
    pub title: String,
    pub url: String,
    /// The window is a private one.
    pub private: bool,
}

/// What the signals say at one moment.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reading {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<App>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab: Option<Tab>,
    /// Seconds since the last keyboard or mouse input.
    pub idle_seconds: u64,
    /// A call window is in front.
    pub call: bool,
    /// The screen is locked or off.
    pub locked: bool,
}

// Never prints a title, an address or a name: only whether each is there.
impl fmt::Debug for Reading {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reading")
            .field("app", &self.app.is_some())
            .field("window_title", &self.window_title.is_some())
            .field("tab", &self.tab.is_some())
            .field("idle_seconds", &self.idle_seconds)
            .field("call", &self.call)
            .field("locked", &self.locked)
            .finish()
    }
}

/// A [`Reading`] and the time it was taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    pub time: SystemTime,
    pub reading: Reading,
}

/// Why the signals could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// A grant the platform asks for is missing; [`Signals::grants`] says which.
    NeedsPermission,
    /// Any other reason, as a short fixed code of the platform's program: no text of the screen.
    Failed(&'static str),
}

/// Whether the person gave one permission the platform asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrantState {
    Granted,
    Denied,
    NotAsked,
}

impl GrantState {
    fn as_str(self) -> &'static str {
        match self {
            GrantState::Granted => "granted",
            GrantState::Denied => "denied",
            GrantState::NotAsked => "not_asked",
        }
    }
}

/// One permission and its state; the name is a fixed word of the platform's program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub name: &'static str,
    pub state: GrantState,
}

/// What the platform gives.
pub trait Signals: Send + Sync {
    /// Whether this computer can give the signals at all. It is asked without a time bound, from
    /// any thread: it must be a constant, cheap answer.
    fn supported(&self) -> bool;

    /// The permissions the signals need, with their states. May block: the sampler bounds it.
    fn grants(&self) -> Vec<Grant>;

    /// The signals now. May block: the sampler bounds it.
    ///
    /// # Errors
    /// [`Failure::NeedsPermission`] when a grant is missing, else [`Failure::Failed`].
    fn read(&self) -> Result<Reading, Failure>;
}

/// The signals of a computer that has none yet: unsupported, so the samples are never taken.
pub struct NoSignals;

impl Signals for NoSignals {
    fn supported(&self) -> bool {
        false
    }

    fn grants(&self) -> Vec<Grant> {
        Vec::new()
    }

    fn read(&self) -> Result<Reading, Failure> {
        Err(Failure::Failed("unsupported"))
    }
}

/// The signals of this computer: the Mac's on a Mac, else none.
#[must_use]
pub fn platform_signals() -> Arc<dyn Signals> {
    #[cfg(target_os = "macos")]
    {
        Arc::new(crate::mac_signals::MacSignals::new())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Arc::new(NoSignals)
    }
}

/// The clock of the sampler, so the tests can move it.
pub trait Clock: Send + Sync {
    fn now(&self) -> SystemTime;
}

/// The system clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// How the last sample went.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// No sample was tried since the agent started or since the switch went on.
    Pending,
    Sampled,
    NeedsPermission,
    Failed(&'static str),
}

/// What the status row shows; the samples are those of the day `day`.
struct Shown {
    outcome: Outcome,
    grants: Vec<Grant>,
    day: String,
    samples_today: u64,
    last_sample: Option<String>,
}

/// One line of a day's file: a run of samples that show the same thing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// The first sample of the run.
    pub time: String,
    /// The last sample of the run.
    pub until: String,
    pub samples: u64,
    #[serde(flatten)]
    pub reading: Reading,
}

/// Told of the open run each time a sample is stored: the line as the file holds it now.
pub type Observer = Arc<dyn Fn(&Line) + Send + Sync>;

/// The lines of a day's file, in order; a torn line is skipped and a missing file has none.
///
/// # Errors
/// Fails when the file cannot be read.
pub fn read_file(path: &Path) -> io::Result<Vec<Line>> {
    read_lines::<Line>(path)
}

/// The run that is the last line of the open file.
struct Run {
    path: PathBuf,
    /// Where the line starts in the file, and how many bytes it has (its newline included).
    offset: u64,
    len: u64,
    line: Line,
    started: SystemTime,
    last_time: SystemTime,
    /// The idle time of the last sample of the run.
    last_idle: u64,
}

impl Run {
    /// Whether `sample` shows what the run shows: the same things in front, no input in between
    /// (the idle time grew as much as the clock did), no longer a gap than `max_gap` since the
    /// last sample (a suspended or stalled computer ends a run) and a run no longer than
    /// [`MAX_RUN`] (so a line lost to a crash is a bounded loss).
    fn continues(&self, sample: &Sample, max_gap: Duration) -> bool {
        let (Ok(gap), Ok(span)) = (
            sample.time.duration_since(self.last_time),
            sample.time.duration_since(self.started),
        ) else {
            return false;
        };
        let reading = &sample.reading;
        let same = reading.app == self.line.reading.app
            && reading.window_title == self.line.reading.window_title
            && reading.tab == self.line.reading.tab
            && reading.call == self.line.reading.call
            && reading.locked == self.line.reading.locked;
        let grew = reading.idle_seconds.checked_sub(self.last_idle);
        same && gap <= max_gap
            && span <= MAX_RUN
            && grew.is_some_and(|grew| grew.abs_diff(gap.as_secs()) <= IDLE_SLACK)
    }
}

/// The files of the samples, and the run that is open.
struct Store {
    data_dir: PathBuf,
    dir: PathBuf,
    max_gap: Duration,
    run: Option<Run>,
}

impl Store {
    fn day_path(&self, day: &str) -> PathBuf {
        self.dir.join(format!("{day}.jsonl"))
    }

    /// Adds `sample` to the file of `day`: to the open run when it continues it and the file is
    /// as the run left it, else as a new line. The open run is replaced once the file is written.
    fn write(&mut self, day: &str, sample: &Sample) -> io::Result<()> {
        let path = self.day_path(day);
        let time = rfc3339(sample.time).ok_or_else(|| io::Error::other("time"))?;
        if let Some(run) = self
            .run
            .as_ref()
            .filter(|r| r.path == path && r.continues(sample, self.max_gap))
            && let Some(done) = rewrite(run, &time, sample)?
        {
            self.run = Some(done);
            return Ok(());
        }
        let new_dir = !self.dir.exists();
        private_dirs(&self.data_dir, &self.dir)?;
        let mut file = private_options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        // A torn last line (a crash in the middle of a write) is left as it is: it is skipped by
        // the readers, and this line starts on a line of its own.
        let mut bytes = Vec::new();
        if !ends_cleanly(&mut file)? {
            bytes.push(b'\n');
        }
        let line = Line {
            time: time.clone(),
            until: time,
            samples: 1,
            reading: sample.reading.clone(),
        };
        let start = file.seek(SeekFrom::End(0))?;
        let offset = start + bytes.len() as u64;
        let encoded = encode(&line)?;
        bytes.extend(&encoded);
        file.write_all(&bytes)?;
        file.sync_data()?;
        // A first write made the folders: their entries are made durable, the data directory last.
        if new_dir || start == 0 {
            sync_chain(&path, &self.data_dir)?;
        }
        self.run = Some(Run {
            path,
            offset,
            len: encoded.len() as u64,
            line,
            started: sample.time,
            last_time: sample.time,
            last_idle: sample.reading.idle_seconds,
        });
        Ok(())
    }

    /// The samples and the time of the last one in the file of `day`. A line that does not read
    /// (a torn one) does not count, and does not hide the others.
    fn read_day(&self, day: &str) -> (u64, Option<String>) {
        read_lines::<Line>(&self.day_path(day))
            .unwrap_or_default()
            .into_iter()
            .fold((0, None), |(count, _), line| {
                (count + line.samples, Some(line.until))
            })
    }
}

/// Writes the run's line again with `sample` in it, over itself, and cuts the file after it (the
/// time of the last sample has no fixed length). `None` when the
/// file is not as the run left it (removed, cut, or longer): the caller starts a line of its own.
fn rewrite(run: &Run, time: &str, sample: &Sample) -> io::Result<Option<Run>> {
    let mut file = match private_options().write(true).open(&run.path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if file.metadata()?.len() != run.offset + run.len {
        return Ok(None);
    }
    let mut line = run.line.clone();
    time.clone_into(&mut line.until);
    line.samples += 1;
    let bytes = encode(&line)?;
    file.seek(SeekFrom::Start(run.offset))?;
    file.write_all(&bytes)?;
    file.set_len(run.offset + bytes.len() as u64)?;
    file.sync_data()?;
    Ok(Some(Run {
        path: run.path.clone(),
        offset: run.offset,
        len: bytes.len() as u64,
        line,
        started: run.started,
        last_time: sample.time,
        last_idle: sample.reading.idle_seconds,
    }))
}

fn encode(line: &Line) -> io::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(line).map_err(io::Error::other)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// The local calendar day of a time, as `YYYY-MM-DD`.
fn local_day(at: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(at)
        .format("%Y-%m-%d")
        .to_string()
}

struct Inner {
    settings: Settings,
    signals: Arc<dyn Signals>,
    clock: Arc<dyn Clock>,
    interval: Duration,
    limit: Duration,
    /// A permit for each call to the signals that may be out: it is given back when the call ends,
    /// also by a panic.
    reads: Arc<Semaphore>,
    grants: Arc<Semaphore>,
    /// Steps taken since the agent started.
    steps: Mutex<u32>,
    store: Mutex<Store>,
    shown: Mutex<Shown>,
    observer: Mutex<Option<Observer>>,
}

/// The sampler and the store of the screen samples. Clones are the same sampler.
#[derive(Clone)]
pub struct Screen(Arc<Inner>);

impl Screen {
    /// A sampler that keeps its files under `data_dir`, on the system clock. Nothing is created
    /// until the first sample.
    #[must_use]
    pub fn new(settings: Settings, data_dir: &Path, signals: Arc<dyn Signals>) -> Self {
        Self::with_timing(
            settings,
            data_dir,
            signals,
            Arc::new(SystemClock),
            INTERVAL,
            CALL_LIMIT,
        )
    }

    /// Like [`Screen::new`], with the clock, the time between samples and the limit of a call given.
    /// The settings answer learns from the signals whether this computer supports the samples.
    #[must_use]
    pub fn with_timing(
        settings: Settings,
        data_dir: &Path,
        signals: Arc<dyn Signals>,
        clock: Arc<dyn Clock>,
        interval: Duration,
        limit: Duration,
    ) -> Self {
        settings.set_screen_supported(signals.supported());
        Self(Arc::new(Inner {
            settings,
            signals,
            clock,
            interval,
            limit,
            reads: Arc::new(Semaphore::new(MAX_STUCK)),
            grants: Arc::new(Semaphore::new(1)),
            steps: Mutex::new(0),
            store: Mutex::new(Store {
                data_dir: data_dir.to_owned(),
                dir: data_dir.join("records").join(SOURCE),
                max_gap: interval * MAX_GAP_INTERVALS,
                run: None,
            }),
            shown: Mutex::new(Shown {
                outcome: Outcome::Pending,
                grants: Vec::new(),
                day: String::new(),
                samples_today: 0,
                last_sample: None,
            }),
            observer: Mutex::new(None),
        }))
    }

    /// Tells `observer` of the open run each time a sample is stored, after it is written.
    pub fn set_observer(&self, observer: Observer) {
        *lock(&self.0.observer) = Some(observer);
    }

    /// Why nothing is sampled now, or `None` when the person's switches are on and the computer
    /// supports it.
    fn off_reason(&self) -> Option<&'static str> {
        let inner = &self.0;
        let c = inner.settings.collection();
        if !c.enabled {
            Some("all collection is off")
        } else if !inner.settings.collects(SOURCE) {
            Some("switched off")
        } else if !inner.signals.supported() {
            Some("not supported on this computer")
        } else {
            None
        }
    }

    /// Runs `f` on a thread of its own and waits for it at most the limit. The error is a fixed
    /// reason: it did not answer in time (the thread is left to finish), it panicked, or too many
    /// earlier calls are still out.
    fn bounded<T: Send + 'static>(
        &self,
        permits: &Arc<Semaphore>,
        f: impl FnOnce(&dyn Signals) -> T + Send + 'static,
    ) -> Result<T, &'static str> {
        let permit = Arc::clone(permits)
            .try_acquire_owned()
            .map_err(|_| "the signals are still stuck")?;
        let (tx, rx) = mpsc::channel();
        let signals = Arc::clone(&self.0.signals);
        thread::spawn(move || {
            let _permit = permit;
            let _ = tx.send(f(signals.as_ref()));
        });
        rx.recv_timeout(self.0.limit).map_err(|e| match e {
            RecvTimeoutError::Timeout => "the signals did not answer in time",
            RecvTimeoutError::Disconnected => "the signals stopped unexpectedly",
        })
    }

    /// Whether the grants are looked at in this step: always while collecting, and once in a
    /// while (and in the first step) otherwise, so the row can tell what the person may grant.
    fn looks_at_grants(&self, collecting: bool) -> bool {
        let mut steps = lock(&self.0.steps);
        let first_or_due = steps.is_multiple_of(GRANTS_EVERY_WHEN_OFF);
        *steps = steps.wrapping_add(1);
        collecting || first_or_due
    }

    /// One step: the grants are looked at, and when the switch is on a sample is taken and stored.
    /// Meant for one thread at a time (the loop of [`Screen::spawn`]); the slots hold the caps
    /// whatever the callers do.
    pub fn tick(&self) {
        let collecting = self.off_reason().is_none();
        if self.looks_at_grants(collecting) {
            // A call that fails leaves no grants: a state that may be stale is not shown.
            let grants = self
                .bounded(&self.0.grants, |signals| signals.grants())
                .unwrap_or_default();
            lock(&self.0.shown).grants = grants;
        }
        let outcome = if collecting {
            self.sample()
        } else {
            Outcome::Pending
        };
        // The one place a step that stored nothing ends the open run.
        if outcome != Outcome::Sampled {
            lock(&self.0.store).run = None;
        }
        lock(&self.0.shown).outcome = outcome;
    }

    /// Reads the signals and stores the sample.
    fn sample(&self) -> Outcome {
        let reading = match self.bounded(&self.0.reads, |signals| signals.read()) {
            Err(reason) => return Outcome::Failed(reason),
            Ok(Err(Failure::NeedsPermission)) => return Outcome::NeedsPermission,
            Ok(Err(Failure::Failed(code))) => return Outcome::Failed(code),
            Ok(Ok(reading)) => reading,
        };
        // Collection may have gone off while the call ran: nothing is stored then.
        if self.off_reason().is_some() {
            return Outcome::Pending;
        }
        let sample = Sample {
            time: self.0.clock.now(),
            reading,
        };
        match self.store(&sample) {
            Ok(()) => Outcome::Sampled,
            Err(e) => {
                eprintln!("screen samples: not written: {}", e.kind());
                Outcome::Failed("the samples could not be written")
            }
        }
    }

    fn store(&self, sample: &Sample) -> io::Result<()> {
        let day = local_day(sample.time);
        let mut store = lock(&self.0.store);
        // The row shows this day, with what its file holds, before the sample is added to it.
        self.show_day(&store, &day);
        store.write(&day, sample)?;
        let open = store.run.as_ref().map(|run| run.line.clone());
        drop(store);
        {
            let mut shown = lock(&self.0.shown);
            shown.samples_today += 1;
            shown.last_sample = rfc3339(sample.time);
        }
        let observer = lock(&self.0.observer).clone();
        if let (Some(observer), Some(line)) = (observer, open) {
            observer(&line);
        }
        Ok(())
    }

    /// Makes the row show `day` with the samples and the last time its file holds, when it shows
    /// another day. The file is read before the row is locked, with the store locked.
    fn show_day(&self, store: &Store, day: &str) {
        if lock(&self.0.shown).day == day {
            return;
        }
        let (count, last) = store.read_day(day);
        let mut shown = lock(&self.0.shown);
        day.clone_into(&mut shown.day);
        shown.samples_today = count;
        shown.last_sample = last;
    }

    /// The `screen` row of the `status` answer.
    #[must_use]
    pub fn status(&self) -> Value {
        let grants = {
            let shown = lock(&self.0.shown);
            shown
                .grants
                .iter()
                .map(|g| json!({ "name": g.name, "state": g.state.as_str() }))
                .collect::<Vec<_>>()
        };
        let today = local_day(self.0.clock.now());
        let (samples_today, last_sample) = self.today(&today);
        let outcome = lock(&self.0.shown).outcome;
        let (state, reason) = match (self.off_reason(), outcome) {
            (Some(reason), _) => ("off", Some(reason)),
            (None, Outcome::NeedsPermission) => {
                ("needs_permission", Some("a permission is missing"))
            }
            (None, Outcome::Failed(code)) => ("failed", Some(code)),
            (None, _) => ("collecting", None),
        };
        let mut row = json!({
            "state": state,
            "samples_today": samples_today,
            "grants": grants,
        });
        if let Some(reason) = reason {
            row["reason"] = reason.into();
        }
        if let Some(last) = last_sample {
            row["last_sample"] = last.into();
        }
        row
    }

    /// The samples of `day` and the time of the last, as the row shows them.
    fn today(&self, day: &str) -> (u64, Option<String>) {
        self.show_day(&lock(&self.0.store), day);
        let shown = lock(&self.0.shown);
        (shown.samples_today, shown.last_sample.clone())
    }

    /// Takes a sample every interval, on a thread of its own, until the returned sender is dropped.
    #[must_use]
    pub fn spawn(&self) -> Sender<()> {
        let (alive, stop) = mpsc::channel::<()>();
        let screen = self.clone();
        thread::spawn(move || {
            loop {
                screen.tick();
                match stop.recv_timeout(screen.0.interval) {
                    Err(RecvTimeoutError::Timeout) => {}
                    _ => break,
                }
            }
        });
        alive
    }
}

impl OwnStore for Screen {
    fn name(&self) -> &'static str {
        "the screen samples"
    }

    fn erase(&self) -> Result<(), String> {
        let mut store = lock(&self.0.store);
        store.run = None;
        // The folder goes the way the other sources go, with its parent made durable.
        let removed = FileSink::new(&store.data_dir)
            .delete_source(SOURCE)
            .map_err(|_| "the screen sample files could not be removed".to_owned());
        let mut shown = lock(&self.0.shown);
        shown.day.clear();
        shown.samples_today = 0;
        shown.last_sample = None;
        removed
    }
}
