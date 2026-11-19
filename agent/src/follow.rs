//! Follows the session files of the coding tools and writes what they say to a [`Sink`].
//!
//! The agent looks at every file by timer, every [`POLL_INTERVAL`], so a new prompt is a
//! record within seconds and the code needs nothing from the platform. A file that was never
//! read and was not changed for 30 days is ignored on every pass; files with
//! a saved offset are always followed. A file whose size and change time are as in the last
//! pass is not opened again. The saved [`Progress`] is the only memory between runs, and the
//! sink drops a replayed record, so a crash between a write and the save loses nothing and
//! doubles nothing. Log files are only opened for reading.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{self, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use connectors::{FileState, Line, Parsed, Record, Registry, Tool, Turn, for_each_line};

use crate::erase::OwnStore;
use crate::records::{FileProgress, FileSink, Progress, Recorder, Sink};

/// A file that was never read is ignored when it was last changed longer ago than this.
const FIRST_SCAN_AGE: Duration = Duration::from_hours(30 * 24);
/// Pause between two passes.
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Lines read before the progress is saved in the middle of a file.
const SAVE_EVERY_LINES: u32 = 5000;
/// The longest the progress stays unsaved while a pass is running.
const SAVE_EVERY: Duration = Duration::from_secs(2);

/// What `status` tells about one tool.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolStatus {
    /// Turns stored since the agent started; a record the sink had stored already is no turn.
    pub turns: u64,
    /// When the last of those turns was stored; none until one is.
    pub last_turn: Option<SystemTime>,
    /// Lines that did not parse or were too long, since the agent started.
    pub skipped: u64,
    /// True from the start until the first pass has gone through this tool's files: listed
    /// them and tried every one (a file that cannot be read does not hold it). It stays true
    /// while the files cannot be listed.
    pub scanning: bool,
    /// Files that could not be read in the last pass.
    pub unreadable: u64,
    /// The user has this tool, or all collection, switched off: its files are followed so the
    /// saved position moves on, and nothing is stored. As of the last pass.
    pub off: bool,
    /// Why this tool is not followed fully: its files cannot be listed (or some directories
    /// cannot), the records cannot be written, the progress cannot be saved, the follower did
    /// not start, or it stopped. Several causes are joined with `; `.
    pub error: Option<String>,
}

impl ToolStatus {
    fn to_json(&self) -> serde_json::Value {
        let mut value = serde_json::json!({
            "turns": self.turns,
            "skipped": self.skipped,
            "scanning": self.scanning,
        });
        if let Some(fields) = value.as_object_mut() {
            if self.off {
                fields.insert("off".into(), true.into());
            }
            if let Some(at) = self.last_turn.and_then(rfc3339) {
                fields.insert("last_turn".into(), at.into());
            }
            if self.unreadable > 0 {
                fields.insert("unreadable".into(), self.unreadable.into());
            }
            if let Some(error) = &self.error {
                fields.insert("error".into(), error.clone().into());
            }
        }
        value
    }
}

/// A time as UTC in RFC 3339, or none when it cannot be written.
fn rfc3339(at: SystemTime) -> Option<String> {
    OffsetDateTime::from(at).format(&Rfc3339).ok()
}

/// [`ToolStatus`] of every tool, shared between the follower and the status answer.
#[derive(Clone, Default)]
pub struct Stats(Arc<Mutex<BTreeMap<String, ToolStatus>>>);

impl Stats {
    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, ToolStatus>> {
        crate::sync::lock(&self.0)
    }

    /// Changes one tool's status. `f` runs under the lock: it must not do I/O.
    fn update(&self, tool: &str, f: impl FnOnce(&mut ToolStatus)) {
        f(self.lock().entry(tool.to_owned()).or_default());
    }

    /// Sets or clears the error of a tool; true when that changes what the status shows
    /// (a new message, or a message cleared). The caller prints it, outside the lock.
    fn set_error(&self, tool: &str, error: Option<String>) -> bool {
        let mut changed = false;
        self.update(tool, |s| {
            changed = s.error != error;
            s.error = error;
        });
        changed
    }

    /// Adds a cause beside the error the tool has already.
    fn add_error(&self, tool: &str, cause: &str) {
        self.update(tool, |s| {
            s.error = Some(match s.error.take() {
                Some(known) => format!("{known}; {cause}"),
                None => cause.to_owned(),
            });
        });
    }

    /// Reports a tool that cannot be followed at all, with the reason.
    pub fn not_started(&self, tool: &str, reason: &str) {
        self.set_error(tool, Some(reason.to_owned()));
    }

    /// The per-tool object of the `status` answer.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let tools = self.lock();
        serde_json::Value::Object(
            tools
                .iter()
                .map(|(name, status)| (name.clone(), status.to_json()))
                .collect(),
        )
    }
}

/// Whether `message` is not the last one told about `key`; it is remembered, so the same
/// message is told once however many passes repeat it.
fn is_new(told: &mut HashMap<String, String>, key: &str, message: &str) -> bool {
    if told.get(key).is_some_and(|m| m == message) {
        return false;
    }
    told.insert(key.to_owned(), message.to_owned());
    true
}

/// Tells the user on stderr. A closed stderr is no reason to stop following the logs, which
/// `eprintln!` would be: it panics.
fn say(message: &str) {
    let _ = writeln!(io::stderr(), "{message}");
}

/// The text of a panic.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_owned())
}

/// The collected sessions as a store the person can have erased (see `erase`): every record
/// file of every tool and the saved positions of the readers. The follower keeps the positions
/// in memory too, so this handle and the follower are made as a pair and the follower is given
/// the handle ([`Follower::with_erase`], [`spawn_observed`]).
///
/// The erasure waits for the pass that runs, removes the files, and tells the follower to forget
/// its positions before its next pass. Until that pass has been through the files of a tool, the
/// tool is read and nothing is stored, whatever the switch says: the session files the tool
/// still has in its own folders are then never collected, not even when collection is switched
/// on again at once. A line written after that pass is.
#[derive(Clone)]
pub struct Erase(Arc<EraseShared>);

struct EraseShared {
    data_dir: PathBuf,
    /// The sources of the tools whose record files this store owns.
    tools: Vec<String>,
    /// Held by the follower for the length of a pass.
    pass: Mutex<()>,
    /// Set by an erasure, taken by the follower at the start of its next pass.
    forget: AtomicBool,
}

impl Erase {
    /// The handle for the follower that stores under `data_dir`. It owns the record files of the
    /// sources `tools` name (the names of the tools it follows) and nothing else under
    /// `records`: another store's files, the chat's conversations, are not its to remove.
    #[must_use]
    pub fn new(data_dir: &Path, tools: &[&str]) -> Self {
        Self(Arc::new(EraseShared {
            data_dir: data_dir.to_owned(),
            tools: tools.iter().map(|t| (*t).to_owned()).collect(),
            pass: Mutex::new(()),
            forget: AtomicBool::new(false),
        }))
    }
}

impl Erase {
    fn marker(&self) -> PathBuf {
        self.0.data_dir.join("erase-settling")
    }

    /// Makes the marker, and the entry in the directory, durable.
    fn write_marker(&self) -> std::io::Result<()> {
        std::fs::File::create(self.marker())?.sync_all()?;
        std::fs::File::open(&self.0.data_dir)?.sync_all()
    }

    fn clear_marker(&self) {
        let _ = std::fs::remove_file(self.marker());
    }
}

impl OwnStore for Erase {
    fn name(&self) -> &'static str {
        "the collected sessions"
    }

    fn erase(&self) -> Result<(), String> {
        let _pass = crate::sync::lock(&self.0.pass);
        // The marker goes first and is made durable: a crash after the files are gone must not
        // leave a restart with neither positions nor marker. At worst a stale marker is left,
        // which only makes the follower read once more without storing.
        match self.write_marker() {
            Ok(()) => {}
            // No data directory: there was nothing collected.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err("the erasure could not be noted, so nothing was removed".to_owned());
            }
        }
        FileSink::new(&self.0.data_dir)
            .delete_collected(&self.0.tools)
            .map_err(|_| "the record files could not be removed".to_owned())?;
        self.0.forget.store(true, Ordering::SeqCst);
        Ok(())
    }
}

/// Tells, by tool name, whether the user lets that tool's sessions be collected. It is asked at
/// before each file is read, so a change of the switch applies from the next file on, not
/// only from the next pass.
pub type Switch = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Called with every user turn a sink stored for the first time, on the writer's thread, after the
/// turn is in the sink. It must not block.
pub type Observer = Arc<dyn Fn(&Turn) + Send + Sync>;

/// Starts a follower of `tools` on a thread of its own, so that neither the first scan nor a
/// slow disk holds up `status`. `paths` is the home directory and the data directory; the
/// follower stops when the returned sender is dropped. When it cannot start (no home, an
/// unreadable progress file), the reason is in the status of every tool.
#[must_use]
pub fn spawn(tools: Registry, paths: io::Result<(PathBuf, PathBuf)>, stats: &Stats) -> Sender<()> {
    spawn_observed(tools, paths, stats, None, None, None, None)
}

/// Like [`spawn`], with a [`Switch`] that decides per tool whether its sessions are stored, an
/// [`Observer`] that is told of every new turn, a [`Recorder`] that is told of every new
/// record and the [`Erase`] handle of the person's erasure.
#[must_use]
pub fn spawn_observed(
    tools: Registry,
    paths: io::Result<(PathBuf, PathBuf)>,
    stats: &Stats,
    switch: Option<Switch>,
    observer: Option<Observer>,
    recorder: Option<Recorder>,
    erase: Option<Erase>,
) -> Sender<()> {
    let (alive, stop) = mpsc::channel::<()>();
    let names: Vec<&'static str> = tools.iter().map(Tool::name).collect();
    let follower = paths
        .and_then(|(home, data)| {
            let mut sink = FileSink::new(&data);
            if let Some(observer) = observer {
                sink = sink.with_observer(observer);
            }
            if let Some(recorder) = recorder {
                sink = sink.with_recorder(recorder);
            }
            Follower::new(tools, &home, sink, stats.clone())
        })
        .map(|follower| match switch {
            Some(switch) => follower.with_switch(switch),
            None => follower,
        })
        .map(|follower| match erase {
            Some(erase) => follower.with_erase(erase),
            None => follower,
        });
    match follower {
        Ok(follower) => {
            let stats = stats.clone();
            std::thread::spawn(move || {
                // A panic would end the thread unseen while `status` kept saying `running`.
                let ended = panic::catch_unwind(AssertUnwindSafe(|| {
                    follower.run(POLL_INTERVAL, &stop);
                }));
                if let Err(payload) = ended {
                    let reason = format!("follower stopped: {}", panic_text(&*payload));
                    say(&format!("connectors: {reason}"));
                    for name in names {
                        stats.not_started(name, &reason);
                    }
                }
            });
        }
        Err(e) => {
            say(&format!("connectors not started: {e}"));
            for name in names {
                stats.not_started(name, &format!("not started: {e}"));
            }
        }
    }
    alive
}

/// Reads the new lines of every file of every tool.
pub struct Follower<S: Sink> {
    tools: Registry,
    home: PathBuf,
    sink: S,
    progress: Progress,
    stats: Stats,
    saver: Saver,
    /// Size and change time of each file as the last pass found it.
    checked: HashMap<String, Checked>,
    /// The last read error of each file that has one.
    read_errors: HashMap<String, String>,
    /// Which tools may be stored; all of them when there is none.
    switch: Option<Switch>,
    /// The person's erasure, when the follower is given one.
    erase: Option<Erase>,
    /// The tools whose files have not been through a pass since an erasure: nothing is stored
    /// from them.
    unsettled: HashSet<&'static str>,
}

type Checked = (u64, Option<SystemTime>);

impl<S: Sink> Follower<S> {
    /// A follower that continues from the progress saved in `sink`. All tools start out
    /// as scanning in `stats`.
    ///
    /// # Errors
    /// Fails when the saved progress cannot be read.
    pub fn new(tools: Registry, home: &Path, sink: S, stats: Stats) -> io::Result<Self> {
        let progress = sink.load_progress()?;
        for tool in tools.iter() {
            stats.update(tool.name(), |s| s.scanning = true);
        }
        Ok(Self {
            tools,
            home: home.to_owned(),
            sink,
            progress,
            stats,
            saver: Saver::new(),
            checked: HashMap::new(),
            read_errors: HashMap::new(),
            switch: None,
            erase: None,
            unsettled: HashSet::new(),
        })
    }

    /// Lets the person's erasure reach this follower: the pass that follows an erasure starts
    /// from nothing read.
    #[must_use]
    pub fn with_erase(mut self, erase: Erase) -> Self {
        // An erasure that no pass has settled yet, before a restart: read once more, store nothing.
        if erase.marker().exists() {
            self.unsettled = self.tools.iter().map(Tool::name).collect();
        }
        self.erase = Some(erase);
        self
    }

    /// Forgets every position, if an erasure said so, and notes that every tool is to be read
    /// once without storing anything.
    fn forget_if_erased(&mut self) {
        let erased = self
            .erase
            .as_ref()
            .is_some_and(|e| e.0.forget.swap(false, Ordering::SeqCst));
        if erased {
            self.progress = Progress::default();
            self.checked.clear();
            self.read_errors.clear();
            self.saver = Saver::new();
            self.sink.forget();
            self.unsettled = self.tools.iter().map(Tool::name).collect();
        }
    }

    /// Store a tool's sessions only while `switch` says so. A tool that is off is still read,
    /// so its saved position moves on and nothing from the time it was off is stored when it
    /// is switched on again; the records of that time are dropped, not kept for later.
    #[must_use]
    pub fn with_switch(mut self, switch: Switch) -> Self {
        self.switch = Some(switch);
        self
    }

    /// Looks at every file once. A tool whose files cannot be listed, or a file that cannot
    /// be read, is shown in `status` and left for the next pass.
    ///
    /// # Errors
    /// Fails when the sink cannot write or the progress cannot be saved; `status` shows it
    /// too, and the next pass tries again.
    // One pass over every tool; split it when a part grows its own logic.
    #[allow(clippy::too_many_lines)]
    pub fn pass(&mut self) -> io::Result<()> {
        // An erasure waits for this lock, so it never comes in the middle of a pass.
        let erase = self.erase.clone();
        let _pass = erase.as_ref().map(|e| crate::sync::lock(&e.0.pass));
        self.forget_if_erased();
        let Self {
            tools,
            home,
            sink,
            progress,
            stats,
            saver,
            checked,
            read_errors,
            switch,
            unsettled,
            ..
        } = self;
        let mut failure = None;
        for tool in tools.iter() {
            let off = !switch.as_ref().is_none_or(|on| on(tool.name()));
            stats.update(tool.name(), |s| s.off = off);
            let root = tool.root(home);
            let listing = match tool.find_files(&root) {
                Ok(listing) => listing,
                Err(e) => {
                    // Told once, not at every pass; `status` shows it for as long as it lasts.
                    let message = format!("cannot list {}: {e}", root.display());
                    if stats.set_error(tool.name(), Some(message.clone())) {
                        say(&format!("{}: {message}", tool.name()));
                    }
                    // No file is being read: the count of an earlier pass is stale.
                    stats.update(tool.name(), |s| s.unreadable = 0);
                    continue;
                }
            };
            let problem = listing.problems.split_first().map(|(first, rest)| {
                if rest.is_empty() {
                    first.clone()
                } else {
                    format!("{first} (and {} more)", rest.len())
                }
            });
            let listed_fully = problem.is_none();
            if stats.set_error(tool.name(), problem.clone())
                && let Some(message) = problem
            {
                say(&format!("{}: {message}", tool.name()));
            }
            let mut unreadable = 0_u64;
            for path in listing.files {
                let mut ctx = Ctx {
                    tool,
                    sink: &mut *sink,
                    progress: &mut *progress,
                    stats,
                    saver: &mut *saver,
                    checked: &mut *checked,
                    switch: switch.as_ref(),
                    unsettled: unsettled.contains(tool.name()),
                };
                let key = path.to_string_lossy().into_owned();
                match ctx.follow(&path) {
                    Ok(()) => {
                        read_errors.remove(&key);
                    }
                    Err(Failure::File(e)) => {
                        unreadable += 1;
                        let message = e.to_string();
                        if is_new(read_errors, &key, &message) {
                            say(&format!("{}: cannot read {key}: {message}", tool.name()));
                        }
                    }
                    Err(Failure::Sink(e)) => {
                        stats.set_error(tool.name(), Some(format!("cannot write records: {e}")));
                        failure = Some(e);
                        break;
                    }
                }
            }
            stats.update(tool.name(), |s| s.unreadable = unreadable);
            if failure.is_some() {
                break;
            }
            // A file that could not be read, or in a directory that could not be listed, has no
            // position yet: until every file can be read the tool stays read-only, and `status`
            // says so.
            if unreadable == 0 && listed_fully {
                unsettled.remove(tool.name());
            } else if unsettled.contains(tool.name()) {
                stats.add_error(
                    tool.name(),
                    "collection waits until every file can be read after the erasure",
                );
            }
            stats.update(tool.name(), |s| s.scanning = false);
        }
        // Kept dirty when it fails, so the next pass saves again.
        let flushed = saver.flush(sink, progress);
        if let Err(e) = &flushed {
            for tool in tools.iter() {
                stats.add_error(tool.name(), &format!("cannot save progress: {e}"));
            }
        }
        // Every tool was read, and the positions are on the disk: the erasure is settled.
        if unsettled.is_empty()
            && flushed.is_ok()
            && failure.is_none()
            && let Some(erase) = &erase
        {
            erase.clear_marker();
        }
        failure.map_or(flushed, Err)
    }

    /// Runs a pass, waits `interval`, and repeats until `stop` is dropped or sent to. A
    /// failing pass is printed once, not at every pass.
    pub fn run(mut self, interval: Duration, stop: &Receiver<()>) {
        let mut told = HashMap::new();
        loop {
            match self.pass() {
                Ok(()) => told.clear(),
                Err(e) => {
                    if is_new(&mut told, "pass", &e.to_string()) {
                        say(&format!("connectors: {e}"));
                    }
                }
            }
            match stop.recv_timeout(interval) {
                Err(RecvTimeoutError::Timeout) => {}
                Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

enum Failure {
    /// This file cannot be read.
    File(io::Error),
    /// The sink cannot write: stop the pass.
    Sink(io::Error),
}

/// Saves the progress now and then, not after every file: each save is a few `fsync`s.
struct Saver {
    lines: u32,
    last: Instant,
    dirty: bool,
}

impl Saver {
    fn new() -> Self {
        Self {
            lines: 0,
            last: Instant::now(),
            dirty: false,
        }
    }

    fn line_done(&mut self) {
        self.lines += 1;
        self.dirty = true;
    }

    fn due(&self) -> bool {
        self.dirty && (self.lines >= SAVE_EVERY_LINES || self.last.elapsed() >= SAVE_EVERY)
    }

    fn flush<S: Sink>(&mut self, sink: &mut S, progress: &Progress) -> io::Result<()> {
        if self.dirty {
            sink.save_progress(progress)?;
        }
        *self = Self::new();
        Ok(())
    }
}

struct Ctx<'a, S: Sink> {
    tool: &'a dyn Tool,
    sink: &'a mut S,
    progress: &'a mut Progress,
    stats: &'a Stats,
    saver: &'a mut Saver,
    checked: &'a mut HashMap<String, Checked>,
    /// Asked before each file is read: while the user has the tool switched off, lines are
    /// read and nothing is stored.
    switch: Option<&'a Switch>,
    /// This tool was not read since an erasure: lines are read and nothing is stored.
    unsettled: bool,
}

impl<S: Sink> Ctx<'_, S> {
    fn follow(&mut self, path: &Path) -> Result<(), Failure> {
        let meta = std::fs::metadata(path).map_err(Failure::File)?;
        let key = path.to_string_lossy().into_owned();
        let seen = (meta.len(), meta.modified().ok());
        if self.checked.get(&key) == Some(&seen) {
            return Ok(());
        }
        let (mut offset, mut state) = match self.progress.files.get(&key) {
            Some(p) if p.offset == meta.len() => {
                self.checked.insert(key, seen);
                return Ok(());
            }
            Some(p) if p.offset < meta.len() => (p.offset, p.state.clone()),
            // A shorter file was replaced: it is a new file, whatever it holds so far. The reset
            // is saved before anything is read, so a file that grows past the old offset is
            // not resumed from there after a restart.
            Some(_) => {
                let state = self.tool.open_file(path);
                self.remember(&key, 0, &state);
                self.saver.dirty = true;
                self.saver
                    .flush(self.sink, self.progress)
                    .map_err(Failure::Sink)?;
                (0, state)
            }
            // After an erasure every file is read once, an old one too, so that its position is
            // saved and a later line in it is the only thing collected.
            None if meta.len() == 0 || (!self.unsettled && Self::too_old(&meta)) => {
                self.checked.insert(key, seen);
                return Ok(());
            }
            None => (0, self.tool.open_file(path)),
        };
        let collect = !self.unsettled && self.switch.is_none_or(|on| on(self.tool.name()));
        let start = offset;
        let mut sink_error = None;
        let read = for_each_line(path, offset, |line, end| {
            let record = match line {
                Line::Text(text) => match self.tool.parse_line(&mut state, &text) {
                    Parsed::Record(r) => Some(r),
                    Parsed::Ignored => None,
                    Parsed::Skipped => {
                        self.count(|s| s.skipped += 1);
                        None
                    }
                },
                Line::TooLong => {
                    self.count(|s| s.skipped += 1);
                    None
                }
            };
            if let Some(record) = record.filter(|_| collect) {
                // Every minute with a log line has a mark, a turn's minute included; the
                // sink keeps one mark per session and minute, so a replay adds nothing.
                if let Record::Turn(turn) = &record
                    && let Err(e) = self.sink.append(&Record::Mark(turn.mark()))
                {
                    sink_error = Some(e);
                    return false;
                }
                match self.sink.append(&record) {
                    // A record the sink had stored already is a replay, not a turn read now.
                    Ok(true) if matches!(record, Record::Turn(_)) => {
                        let now = SystemTime::now();
                        self.count(|s| {
                            s.turns += 1;
                            s.last_turn = Some(now);
                        });
                    }
                    Ok(_) => {}
                    Err(e) => {
                        sink_error = Some(e);
                        return false;
                    }
                }
            }
            offset = end;
            self.saver.line_done();
            if self.saver.due() {
                self.remember(&key, offset, &state);
                if let Err(e) = self.saver.flush(self.sink, self.progress) {
                    sink_error = Some(e);
                    return false;
                }
            }
            true
        });
        // What was read stays read, even when the loop ended on an error.
        if offset != start {
            self.remember(&key, offset, &state);
        }
        if let Some(e) = sink_error {
            return Err(Failure::Sink(e));
        }
        read.map_err(Failure::File)?;
        self.checked.insert(key, seen);
        Ok(())
    }

    fn remember(&mut self, key: &str, offset: u64, state: &FileState) {
        self.progress.files.insert(
            key.to_owned(),
            FileProgress {
                offset,
                state: state.clone(),
            },
        );
    }

    fn too_old(meta: &std::fs::Metadata) -> bool {
        meta.modified()
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age > FIRST_SCAN_AGE)
    }

    fn count(&self, f: impl FnOnce(&mut ToolStatus)) {
        self.stats.update(self.tool.name(), f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_is_new_once_and_again_after_the_entry_is_cleared() {
        let mut told = HashMap::new();
        assert!(is_new(&mut told, "a", "denied"));
        assert!(!is_new(&mut told, "a", "denied"));
        assert!(is_new(&mut told, "a", "gone"));
        assert!(is_new(&mut told, "b", "gone"));
        told.remove("a");
        assert!(is_new(&mut told, "a", "gone"));
    }
}
