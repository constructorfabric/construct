//! The follower on temporary directories, with a tool of its own: lines `bad` do not parse,
//! lines starting with `#` are of no interest, anything else is a turn.

#![cfg(unix)]
// The helpers sit outside `#[test]` functions, where `clippy.toml` does not allow `unwrap`.
#![allow(clippy::unwrap_used)]

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, SystemTime};

use connectors::{
    ClaudeCode, Codex, FileState, Listing, Parsed, Record, Registry, Role, Tool, Turn,
    MAX_LINE_BYTES,
};
use construct_agent::follow::{self, Follower, Stats};
use construct_agent::ipc::{query_status, Server};
use construct_agent::records::{FileSink, Sink};

/// How the fake tool lists its files.
enum List {
    /// Every entry of its directory.
    Files,
    /// Always fails.
    Fails,
    /// Fails on the first call only.
    FailsOnce(AtomicBool),
    /// Waits for a message before every listing: a scan that takes long.
    Waits(Mutex<mpsc::Receiver<()>>),
    /// Lists the files, and says one directory could not be read.
    WithProblem,
    /// Fails while the flag is set.
    Switch(Arc<AtomicBool>),
    /// Panics.
    Panics,
}

struct Fake {
    list: List,
    /// How many times a file was opened for the first time.
    opened: Arc<AtomicUsize>,
}

impl Fake {
    fn new(list: List) -> Self {
        Self {
            list,
            opened: Arc::default(),
        }
    }
}

impl Tool for Fake {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn root(&self, home: &Path) -> PathBuf {
        home.join(".fake")
    }

    fn find_files(&self, root: &Path) -> io::Result<Listing> {
        let mut listing = Listing::default();
        match &self.list {
            List::Fails => return Err(denied()),
            List::FailsOnce(failed) if !failed.swap(true, Ordering::SeqCst) => {
                return Err(denied());
            }
            List::Waits(release) => {
                let _ = release.lock().unwrap().recv();
            }
            List::WithProblem => listing.problems.push("cannot list /x/y: denied".into()),
            List::Switch(broken) if broken.load(Ordering::SeqCst) => return Err(denied()),
            List::Panics => panic!("the listing blew up"),
            List::Files | List::FailsOnce(_) | List::Switch(_) => {}
        }
        match fs::read_dir(root) {
            Ok(dir) => {
                for f in dir {
                    listing.files.push(f?.path());
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        listing.files.sort();
        Ok(listing)
    }

    fn open_file(&self, path: &Path) -> FileState {
        self.opened.fetch_add(1, Ordering::SeqCst);
        FileState {
            session: path.file_stem().unwrap().to_string_lossy().into_owned(),
            ..FileState::default()
        }
    }

    fn parse_line(&self, state: &mut FileState, line: &str) -> Parsed {
        if line == "bad" {
            return Parsed::Skipped;
        }
        if line.starts_with('#') {
            return Parsed::Ignored;
        }
        Parsed::Record(Record::Turn(Turn {
            tool: "fake".into(),
            session: state.session.clone(),
            time: "2026-11-11T10:00:00Z".into(),
            folder: "/w".into(),
            branch: String::new(),
            role: Role::User,
            text: line.into(),
        }))
    }
}

fn denied() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "denied")
}

struct Setup {
    root: PathBuf,
    home: PathBuf,
    data: PathBuf,
}

impl Setup {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("follow-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let home = root.join("home");
        fs::create_dir_all(home.join(".fake")).unwrap();
        Self {
            data: root.join("data"),
            home,
            root,
        }
    }

    fn log(&self, name: &str) -> PathBuf {
        self.home.join(".fake").join(name)
    }

    /// A follower of `registry` as a fresh agent start makes it: nothing but the saved
    /// progress carries over.
    fn follower_of(&self, registry: Registry, stats: &Stats) -> Follower<FileSink> {
        Follower::new(
            registry,
            &self.home,
            FileSink::new(&self.data),
            stats.clone(),
        )
        .unwrap()
    }

    fn follower_with(&self, tool: Fake, stats: &Stats) -> Follower<FileSink> {
        self.follower_of(Registry::new(vec![Box::new(tool)]), stats)
    }

    fn follower(&self, stats: &Stats) -> Follower<FileSink> {
        self.follower_with(Fake::new(List::Files), stats)
    }

    fn texts(&self, session: &str) -> Vec<String> {
        FileSink::new(&self.data)
            .turns_before("fake", session, "9999", 1000)
            .unwrap()
            .into_iter()
            .map(|t| t.text)
            .collect()
    }
}

fn append(path: &Path, text: &str) {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
}

fn tool_status(stats: &Stats) -> serde_json::Value {
    stats.to_json()["fake"].clone()
}

#[test]
fn a_restart_in_the_middle_of_a_growing_file_gives_no_duplicate_and_no_gap() {
    let s = Setup::new("restart");
    let log = s.log("a.log");
    append(&log, "bad\none\ntwo\n");
    let stats = Stats::default();
    s.follower(&stats).pass().unwrap();
    assert_eq!(s.texts("a"), ["one", "two"]);

    append(&log, "three\nfour\n");
    let before = fs::read(&log).unwrap();
    let restarted_stats = Stats::default();
    let mut restarted = s.follower(&restarted_stats);
    restarted.pass().unwrap();
    restarted.pass().unwrap();
    assert_eq!(s.texts("a"), ["one", "two", "three", "four"]);
    // Only the new lines were read: the `bad` line at the top was not read a second time.
    assert_eq!(
        tool_status(&restarted_stats),
        serde_json::json!({ "turns": 2, "skipped": 0, "scanning": false })
    );
    assert_eq!(fs::read(&log).unwrap(), before, "the log is never changed");
}

#[test]
fn a_restart_after_a_crash_before_the_progress_was_saved_adds_nothing_twice() {
    let s = Setup::new("crash");
    append(&s.log("a.log"), "one\ntwo\n");
    s.follower(&Stats::default()).pass().unwrap();
    let progress = s.data.join("connectors-progress.json");
    let saved_before = fs::read(&progress).unwrap();

    append(&s.log("a.log"), "three\n");
    s.follower(&Stats::default()).pass().unwrap();
    // The records were written, the crash came before the offset was.
    fs::write(&progress, saved_before).unwrap();
    let stats = Stats::default();
    s.follower(&stats).pass().unwrap();
    assert_eq!(s.texts("a"), ["one", "two", "three"]);
    // The turns read again are replays, not new turns.
    assert_eq!(tool_status(&stats)["turns"], 0);
}

#[test]
fn a_line_that_is_still_being_written_is_read_once_it_is_complete() {
    let s = Setup::new("torn");
    let log = s.log("a.log");
    append(&log, "one\ntw");
    let mut follower = s.follower(&Stats::default());
    follower.pass().unwrap();
    assert_eq!(s.texts("a"), ["one"]);
    append(&log, "o");
    follower.pass().unwrap();
    assert_eq!(s.texts("a"), ["one"]);
    append(&log, "\n");
    follower.pass().unwrap();
    follower.pass().unwrap();
    assert_eq!(s.texts("a"), ["one", "two"]);
}

#[test]
fn a_file_that_gets_shorter_is_read_again_from_the_start() {
    let s = Setup::new("shorter");
    let log = s.log("a.log");
    append(&log, "first line\nsecond line\n");
    let mut follower = s.follower(&Stats::default());
    follower.pass().unwrap();
    fs::write(&log, "new\n").unwrap();
    follower.pass().unwrap();
    assert_eq!(s.texts("a"), ["first line", "second line", "new"]);
}

#[test]
fn files_older_than_30_days_are_not_read_on_first_start() {
    let s = Setup::new("old");
    append(&s.log("old.log"), "ancient\n");
    append(&s.log("new.log"), "fresh\n");
    let forty_days = Duration::from_hours(40 * 24);
    File::options()
        .write(true)
        .open(s.log("old.log"))
        .unwrap()
        .set_modified(SystemTime::now() - forty_days)
        .unwrap();
    s.follower(&Stats::default()).pass().unwrap();
    assert!(s.texts("old").is_empty());
    assert_eq!(s.texts("new"), ["fresh"]);
}

#[test]
fn a_missing_root_is_not_an_error_and_the_scan_is_over() {
    let s = Setup::new("missing");
    fs::remove_dir_all(s.home.join(".fake")).unwrap();
    let stats = Stats::default();
    let mut follower = s.follower(&stats);
    assert_eq!(tool_status(&stats)["scanning"], true);
    follower.pass().unwrap();
    assert_eq!(
        tool_status(&stats),
        serde_json::json!({ "turns": 0, "skipped": 0, "scanning": false })
    );
    // The tool is installed later: its files are found by the next pass.
    fs::create_dir_all(s.home.join(".fake")).unwrap();
    append(&s.log("a.log"), "hello\n");
    follower.pass().unwrap();
    assert_eq!(s.texts("a"), ["hello"]);
}

#[test]
fn turns_and_skipped_lines_are_counted() {
    let s = Setup::new("counts");
    append(&s.log("a.log"), "one\nbad\n# nothing\ntwo\nbad\n");
    let stats = Stats::default();
    s.follower(&stats).pass().unwrap();
    assert_eq!(
        tool_status(&stats),
        serde_json::json!({ "turns": 2, "skipped": 2, "scanning": false })
    );
}

#[test]
fn a_file_with_a_saved_offset_is_followed_however_old_it_is() {
    let s = Setup::new("followed");
    let log = s.log("a.log");
    append(&log, "one\n");
    s.follower(&Stats::default()).pass().unwrap();
    let forty_days = Duration::from_hours(40 * 24);
    append(&log, "two\n");
    File::options()
        .write(true)
        .open(&log)
        .unwrap()
        .set_modified(SystemTime::now() - forty_days)
        .unwrap();
    s.follower(&Stats::default()).pass().unwrap();
    assert_eq!(s.texts("a"), ["one", "two"]);
}

#[test]
fn a_file_truncated_to_empty_and_grown_past_the_old_offset_is_read_from_the_start() {
    let s = Setup::new("truncated");
    let log = s.log("a.log");
    append(&log, "aaaa\nbbbb\ncccc\n");
    let mut follower = s.follower(&Stats::default());
    follower.pass().unwrap();
    fs::write(&log, "").unwrap();
    follower.pass().unwrap();
    fs::write(&log, "x1\nx2\nx3\nx4\nx5\nx6\n").unwrap();
    follower.pass().unwrap();
    assert_eq!(
        s.texts("a"),
        ["aaaa", "bbbb", "cccc", "x1", "x2", "x3", "x4", "x5", "x6"]
    );
}

#[test]
fn a_root_that_cannot_be_listed_is_not_a_finished_scan() {
    let s = Setup::new("broken");
    let stats = Stats::default();
    let mut follower = s.follower_with(Fake::new(List::Fails), &stats);
    for _ in 0..2 {
        follower.pass().unwrap();
        let seen = tool_status(&stats);
        assert_eq!(seen["scanning"], true);
        assert!(seen["error"].as_str().unwrap().contains("denied"), "{seen}");
    }
}

#[test]
fn a_directory_that_cannot_be_listed_does_not_hide_the_other_files() {
    let s = Setup::new("problem");
    append(&s.log("a.log"), "one\n");
    let stats = Stats::default();
    s.follower_with(Fake::new(List::WithProblem), &stats)
        .pass()
        .unwrap();
    assert_eq!(s.texts("a"), ["one"]);
    let seen = tool_status(&stats);
    assert_eq!(seen["error"], "cannot list /x/y: denied");
    assert_eq!(seen["scanning"], false);
}

#[test]
fn what_is_copied_from_the_logs_is_for_the_user_only() {
    let s = Setup::new("modes");
    // Directories that exist already, as an older version or the user made them.
    for dir in [s.data.clone(), s.data.join("records/fake")] {
        fs::create_dir_all(&dir).unwrap();
    }
    for dir in [
        &s.data,
        &s.data.join("records"),
        &s.data.join("records/fake"),
    ] {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap();
    }
    append(&s.log("a.log"), "private\n");
    s.follower(&Stats::default()).pass().unwrap();
    let mode = |p: PathBuf| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(s.data.clone()), 0o700);
    assert_eq!(mode(s.data.join("records")), 0o700);
    assert_eq!(mode(s.data.join("records").join("fake")), 0o700);
    assert_eq!(mode(s.data.join("records/fake/a.jsonl")), 0o600);
    assert_eq!(mode(s.data.join("connectors-progress.json")), 0o600);
}

#[test]
fn the_real_tools_count_what_they_skip() {
    let s = Setup::new("real");
    let claude = s.home.join(".claude/projects/project/sess-1.jsonl");
    let codex = s.home.join(".codex/sessions/2026/11/11");
    let codex_log =
        codex.join("rollout-2026-11-11T10-15-00-01a0bee2-6260-73a3-b389-43c86bb8930f.jsonl");
    fs::create_dir_all(claude.parent().unwrap()).unwrap();
    fs::create_dir_all(&codex).unwrap();

    let user = |text: &str| {
        format!(
            r#"{{"type":"user","timestamp":"2026-11-11T10:15:30.123Z","cwd":"/w","sessionId":"sess-1","message":{{"role":"user","content":"{text}"}}}}"#
        )
    };
    let huge = format!(
        r#"{{"type":"user","pad":"{}"}}"#,
        "x".repeat(MAX_LINE_BYTES)
    );
    // A line of a sub-agent is a mark, not a turn.
    let side = user("side").replace(r#""type":"user","#, r#""type":"user","isSidechain":true,"#);
    append(
        &claude,
        &format!(
            "{}\n{{broken\n{huge}\n{}\n{side}\n",
            user("one"),
            user("two")
        ),
    );
    let item = |kind: &str, payload: &str| {
        format!(r#"{{"timestamp":"2026-11-11T10:15:30.123Z","type":"{kind}","payload":{payload}}}"#)
    };
    append(
        &codex_log,
        &format!(
            "{}\n{{broken\n{}\n{}\n",
            item(
                "session_meta",
                r#"{"id":"01a0bee2-6260-73a3-b389-43c86bb8930f","cwd":"/w"}"#
            ),
            item(
                "response_item",
                r#"{"type":"message","role":"user","content":[{"type":"input_text","text":"fix it"}]}"#
            ),
            item("event_msg", r#"{"type":"token_count"}"#),
        ),
    );

    let stats = Stats::default();
    // The real tools, reading under the temporary home whatever `CLAUDE_CONFIG_DIR` and
    // `CODEX_HOME` say on the machine that runs the test.
    let registry = Registry::new(vec![
        Box::new(Under(ClaudeCode, ".claude/projects")),
        Box::new(Under(Codex, ".codex/sessions")),
    ]);
    s.follower_of(registry, &stats).pass().unwrap();
    let all = stats.to_json();
    assert_eq!(
        all["claude-code"],
        serde_json::json!({ "turns": 2, "skipped": 2, "scanning": false })
    );
    assert_eq!(
        all["codex"],
        serde_json::json!({ "turns": 1, "skipped": 1, "scanning": false })
    );
    // The marks were stored, and they are not turns.
    for (tool, session) in [
        ("claude-code", "sess-1"),
        ("codex", "01a0bee2-6260-73a3-b389-43c86bb8930f"),
    ] {
        let stored =
            fs::read_to_string(s.data.join(format!("records/{tool}/{session}.jsonl"))).unwrap();
        assert_eq!(stored.matches(r#""kind":"mark""#).count(), 1, "{tool}");
    }
}

/// A real tool that looks for its files under a directory of the test's home instead of
/// the one its environment variable names; everything else is the tool's own.
struct Under<T>(T, &'static str);

impl<T: Tool> Tool for Under<T> {
    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn root(&self, home: &Path) -> PathBuf {
        home.join(self.1)
    }

    fn find_files(&self, root: &Path) -> io::Result<Listing> {
        self.0.find_files(root)
    }

    fn open_file(&self, path: &Path) -> FileState {
        self.0.open_file(path)
    }

    fn parse_line(&self, state: &mut FileState, line: &str) -> Parsed {
        self.0.parse_line(state, line)
    }
}

#[test]
fn a_listing_error_goes_away_when_the_root_can_be_listed_again() {
    let s = Setup::new("flaky");
    let stats = Stats::default();
    let mut follower = s.follower_with(Fake::new(List::FailsOnce(false.into())), &stats);
    follower.pass().unwrap();
    assert!(tool_status(&stats)["error"].as_str().is_some());
    assert_eq!(tool_status(&stats)["scanning"], true);
    follower.pass().unwrap();
    assert_eq!(
        tool_status(&stats),
        serde_json::json!({ "turns": 0, "skipped": 0, "scanning": false })
    );
}

#[test]
fn a_file_that_cannot_be_read_is_counted_until_it_can_be_read() {
    let s = Setup::new("unreadable");
    // A directory where a log should be: it opens, and reading it fails, also for root.
    fs::create_dir(s.log("a.log")).unwrap();
    append(&s.log("b.log"), "fine\n");
    let stats = Stats::default();
    let mut follower = s.follower(&stats);
    for _ in 0..2 {
        follower.pass().unwrap();
        assert_eq!(tool_status(&stats)["unreadable"], 1);
    }
    assert_eq!(s.texts("b"), ["fine"]);
    // The log is a file now: it is read, and nothing is unreadable any more.
    fs::remove_dir(s.log("a.log")).unwrap();
    append(&s.log("a.log"), "back\n");
    follower.pass().unwrap();
    assert!(tool_status(&stats).get("unreadable").is_none());
    assert_eq!(s.texts("a"), ["back"]);
}

#[test]
fn a_sink_that_fails_stops_the_pass_and_nothing_is_lost_or_doubled() {
    let s = Setup::new("sink");
    append(&s.log("a.log"), "one\ntwo\n");
    append(&s.log("b.log"), "three\n");
    // `records` is a file, so no record can be written.
    fs::create_dir_all(&s.data).unwrap();
    fs::write(s.data.join("records"), "").unwrap();
    let stats = Stats::default();
    let mut follower = s.follower(&stats);
    assert!(follower.pass().is_err());
    let seen = tool_status(&stats);
    assert!(
        seen["error"]
            .as_str()
            .unwrap()
            .starts_with("cannot write records"),
        "{seen}"
    );
    assert_eq!(seen["scanning"], true);
    fs::remove_file(s.data.join("records")).unwrap();
    follower.pass().unwrap();
    assert_eq!(s.texts("a"), ["one", "two"]);
    assert_eq!(s.texts("b"), ["three"]);
    assert_eq!(
        tool_status(&stats),
        serde_json::json!({ "turns": 3, "skipped": 0, "scanning": false })
    );
}

#[test]
fn a_file_that_did_not_change_is_not_opened_again() {
    let s = Setup::new("checked");
    // Never read, ending in a line that is not complete: nothing to read, and nothing saved.
    append(&s.log("a.log"), "torn");
    let tool = Fake::new(List::Files);
    let opened = tool.opened.clone();
    let mut follower = s.follower_with(tool, &Stats::default());
    follower.pass().unwrap();
    follower.pass().unwrap();
    assert_eq!(opened.load(Ordering::SeqCst), 1);
    append(&s.log("a.log"), "\n");
    follower.pass().unwrap();
    assert_eq!(s.texts("a"), ["torn"]);
}

#[test]
fn an_agent_that_cannot_read_its_progress_says_so_in_status_of_every_tool() {
    let s = Setup::new("corrupt");
    fs::create_dir_all(&s.data).unwrap();
    fs::write(s.data.join("connectors-progress.json"), "{not json").unwrap();
    let stats = Stats::default();
    let registry = Registry::new(vec![
        Box::new(Fake::new(List::Files)),
        Box::new(Under(Codex, ".codex/sessions")),
    ]);
    let alive = follow::spawn(registry, Ok((s.home.clone(), s.data.clone())), &stats);
    let all = stats.to_json();
    for tool in ["fake", "codex"] {
        let error = all[tool]["error"].as_str().unwrap();
        assert!(error.starts_with("not started: "), "{tool}: {error}");
    }
    drop(alive);
}

#[test]
fn the_first_scan_does_not_block_status() {
    let s = Setup::new("status");
    let (release, wait) = mpsc::channel();
    let stats = Stats::default();
    let follower = s.follower_with(Fake::new(List::Waits(Mutex::new(wait))), &stats);
    let (stop, stopped) = mpsc::channel();
    let thread = std::thread::spawn(move || follower.run(Duration::from_millis(20), &stopped));

    let socket = s.root.join("run").join("agent.sock");
    let server = Server::bind(&socket)
        .unwrap()
        .with_connectors(stats.clone());
    std::thread::spawn(move || server.serve());

    let reply: serde_json::Value = serde_json::from_str(&query_status(&socket).unwrap()).unwrap();
    assert_eq!(reply["status"], "running");
    assert_eq!(reply["connectors"]["fake"]["scanning"], true);

    release.send(()).unwrap();
    let mut done = false;
    for _ in 0..200 {
        let reply: serde_json::Value =
            serde_json::from_str(&query_status(&socket).unwrap()).unwrap();
        if reply["connectors"]["fake"]["scanning"] == false {
            done = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(done, "the scan never finished");
    // The fake waits on `release` at every pass: it must be gone for the thread to see `stop`.
    drop(release);
    drop(stop);
    thread.join().unwrap();
}

#[test]
fn a_listing_failure_clears_the_unreadable_count_of_the_pass_before() {
    let s = Setup::new("unreadable-gone");
    fs::create_dir(s.log("a.log")).unwrap();
    let broken = Arc::new(AtomicBool::new(false));
    let stats = Stats::default();
    let mut follower = s.follower_with(Fake::new(List::Switch(broken.clone())), &stats);
    follower.pass().unwrap();
    assert_eq!(tool_status(&stats)["unreadable"], 1);
    broken.store(true, Ordering::SeqCst);
    follower.pass().unwrap();
    let seen = tool_status(&stats);
    assert!(seen.get("unreadable").is_none(), "{seen}");
    assert!(seen["error"].as_str().is_some(), "{seen}");
}

#[test]
fn a_progress_that_cannot_be_saved_is_shown_beside_the_other_error_and_saved_later() {
    let s = Setup::new("save");
    append(&s.log("a.log"), "one\ntwo\n");
    let stats = Stats::default();
    let mut follower = s.follower_with(Fake::new(List::WithProblem), &stats);
    // A directory where the progress file goes: the file cannot be replaced.
    let progress = s.data.join("connectors-progress.json");
    fs::create_dir_all(&progress).unwrap();
    assert!(follower.pass().is_err());
    let error = tool_status(&stats)["error"].as_str().unwrap().to_owned();
    assert!(
        error.starts_with("cannot list /x/y: denied; cannot save progress"),
        "{error}"
    );
    assert_eq!(s.texts("a"), ["one", "two"]);

    fs::remove_dir_all(&progress).unwrap();
    follower.pass().unwrap();
    assert_eq!(tool_status(&stats)["error"], "cannot list /x/y: denied");
    // The retry saved it: a restart has nothing left to read.
    let restarted = Stats::default();
    s.follower(&restarted).pass().unwrap();
    assert_eq!(tool_status(&restarted)["turns"], 0);
    assert!(progress.is_file());
}

/// Writes through to a real sink until it is told to crash: `appends` more records, then
/// `saves` more saves of the progress succeed, and every one after fails.
struct Crashing {
    inner: FileSink,
    appends: usize,
    saves: usize,
}

impl Sink for Crashing {
    fn append(&mut self, record: &Record) -> io::Result<bool> {
        self.appends = self.appends.checked_sub(1).ok_or_else(denied)?;
        self.inner.append(record)
    }

    fn turns_before(
        &self,
        tool: &str,
        session: &str,
        before: &str,
        limit: usize,
    ) -> io::Result<Vec<Turn>> {
        self.inner.turns_before(tool, session, before, limit)
    }

    fn save_progress(&mut self, progress: &construct_agent::records::Progress) -> io::Result<()> {
        self.saves = self.saves.checked_sub(1).ok_or_else(denied)?;
        self.inner.save_progress(progress)
    }

    fn load_progress(&self) -> io::Result<construct_agent::records::Progress> {
        self.inner.load_progress()
    }
}

#[test]
fn a_crash_in_a_long_first_scan_keeps_what_the_checkpoint_saved() {
    let s = Setup::new("checkpoint");
    let log = s.log("a.log");
    // 10 bytes a line; the checkpoint comes after 5000 lines.
    let lines = (0..6000)
        .map(|i| format!("line{i:05}\n"))
        .collect::<Vec<_>>()
        .concat();
    append(&log, &lines);
    let sink = Crashing {
        inner: FileSink::new(&s.data),
        appends: 5200,
        saves: 1,
    };
    let registry = Registry::new(vec![Box::new(Fake::new(List::Files))]);
    let mut follower = Follower::new(registry, &s.home, sink, Stats::default()).unwrap();
    // The sink fails at line 5200, and so does the save at the end of the pass: a crash.
    assert!(follower.pass().is_err());
    let saved = FileSink::new(&s.data).load_progress().unwrap();
    let offset = saved
        .files
        .get(&log.to_string_lossy().into_owned())
        .map(|f| f.offset);
    assert_eq!(offset, Some(5000 * 10));

    let stats = Stats::default();
    s.follower(&stats).pass().unwrap();
    // The 200 lines after the checkpoint were stored already: only 5200..6000 are new.
    assert_eq!(tool_status(&stats)["turns"], 800);
    let tail = s.texts("a");
    assert_eq!(tail.len(), 1000);
    assert_eq!(tail[999], "line05999");
}

#[test]
fn a_restart_after_a_shrink_and_before_the_file_grows_again_loses_nothing() {
    let s = Setup::new("shrink-restart");
    let log = s.log("a.log");
    append(&log, "aaaaaaaa\nbbbbbbbb\ncccccccc\n");
    s.follower(&Stats::default()).pass().unwrap();
    fs::write(&log, "new\n").unwrap();
    s.follower(&Stats::default()).pass().unwrap();
    // A fresh agent now; the file grows past the old size.
    append(&log, "x1\nx2\nx3\nx4\nx5\nx6\nx7\nx8\n");
    s.follower(&Stats::default()).pass().unwrap();
    assert_eq!(
        s.texts("a"),
        [
            "aaaaaaaa", "bbbbbbbb", "cccccccc", "new", "x1", "x2", "x3", "x4", "x5", "x6", "x7",
            "x8"
        ]
    );
}

#[test]
fn a_follower_that_panics_says_so_in_status_of_every_tool() {
    let s = Setup::new("panic");
    let stats = Stats::default();
    let registry = Registry::new(vec![Box::new(Fake::new(List::Panics))]);
    let _alive = follow::spawn(registry, Ok((s.home.clone(), s.data.clone())), &stats);
    for _ in 0..200 {
        if let Some(error) = tool_status(&stats)["error"].as_str() {
            assert_eq!(error, "follower stopped: the listing blew up");
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("the panic never showed in status");
}

#[test]
fn a_tool_that_is_switched_off_is_read_but_nothing_is_stored_and_nothing_comes_back_later() {
    let s = Setup::new("switch");
    let log = s.log("a.log");
    append(&log, "one\ntwo\n");
    let on = Arc::new(AtomicBool::new(false));
    let switch: follow::Switch = {
        let on = on.clone();
        Arc::new(move |tool| tool == "fake" && on.load(Ordering::SeqCst))
    };
    let stats = Stats::default();
    let mut follower = s.follower(&stats).with_switch(switch);

    // Off from the start: the file is read, nothing is stored or counted.
    follower.pass().unwrap();
    append(&log, "three\n");
    follower.pass().unwrap();
    assert!(s.texts("a").is_empty());
    assert_eq!(tool_status(&stats)["turns"], 0);
    assert_eq!(tool_status(&stats)["scanning"], false);

    // A file that appears while it is off is not kept for later either.
    append(&s.log("b.log"), "other\n");
    follower.pass().unwrap();
    assert!(s.texts("b").is_empty());

    // Switched on: only what is written from now on is stored.
    on.store(true, Ordering::SeqCst);
    append(&log, "four\n");
    append(&s.log("b.log"), "five\n");
    follower.pass().unwrap();
    assert_eq!(s.texts("a"), ["four"]);
    assert_eq!(s.texts("b"), ["five"]);

    // Off again, also after a restart: what is written meanwhile does not come back.
    on.store(false, Ordering::SeqCst);
    append(&log, "six\n");
    follower.pass().unwrap();
    on.store(true, Ordering::SeqCst);
    let restarted = Stats::default();
    s.follower(&restarted)
        .with_switch(Arc::new(|_| true))
        .pass()
        .unwrap();
    assert_eq!(s.texts("a"), ["four"]);
}

#[test]
fn the_switch_is_asked_for_every_file_so_a_long_scan_stops_when_it_is_turned_off() {
    let s = Setup::new("switch-per-file");
    append(&s.log("a.log"), "one\n");
    append(&s.log("b.log"), "two\n");
    // On for the first file of the pass only: the user turns collection off during the scan.
    let asked = Arc::new(AtomicUsize::new(0));
    let switch: follow::Switch = {
        let asked = asked.clone();
        Arc::new(move |_| asked.fetch_add(1, Ordering::SeqCst) == 0)
    };
    let stats = Stats::default();
    s.follower(&stats).with_switch(switch).pass().unwrap();
    assert_eq!(s.texts("a"), ["one"]);
    assert!(s.texts("b").is_empty());
    assert_eq!(asked.load(Ordering::SeqCst), 2);
}
