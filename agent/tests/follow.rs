//! The follower on temporary directories, with a tool of its own: lines `bad` do not parse,
//! lines starting with `#` are of no interest, anything else is a turn.

#![cfg(unix)]
#![allow(clippy::unwrap_used)]

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, SystemTime};

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use connectors::{
    Author, ClaudeCode, Codex, FileState, Listing, MAX_LINE_BYTES, Parsed, Record, Registry, Role,
    Tool, Turn,
};
use construct_agent::follow::{self, Follower, Stats};
use construct_agent::ipc::{Server, query_status};
use construct_agent::records::{FileSink, Sink};

mod common;

use common::tools::{Under, claude_user, codex_item, codex_meta, codex_user};

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

    /// Installed unless the home holds `.fake-uninstalled`.
    fn installed(&self, home: &Path) -> bool {
        !home.join(".fake-uninstalled").exists()
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
            author: Author::Unknown,
            text: line.into(),
            tools: Vec::new(),
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
        self.texts_up_to(session, 1000)
    }

    /// The last `limit` turns stored for a session, oldest first.
    fn texts_up_to(&self, session: &str, limit: usize) -> Vec<String> {
        FileSink::new(&self.data)
            .turns_before("fake", session, "9999", limit)
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

/// The status of a tool without its `last_turn`, which holds the time of the run.
fn without_last_turn(mut status: serde_json::Value) -> serde_json::Value {
    status.as_object_mut().unwrap().remove("last_turn");
    status
}

fn parse_time(text: &str) -> SystemTime {
    OffsetDateTime::parse(text, &Rfc3339).unwrap().into()
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
        without_last_turn(tool_status(&restarted_stats)),
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
    // The turns read again are replays, not new turns, and they do not move `last_turn`.
    let shown = tool_status(&stats);
    assert_eq!(shown["turns"], 0);
    assert!(shown.get("last_turn").is_none());
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
fn a_tool_whose_app_is_not_installed_says_so_and_no_more_once_it_is() {
    let s = Setup::new("not-installed");
    fs::write(s.home.join(".fake-uninstalled"), "").unwrap();
    let stats = Stats::default();
    let mut follower = s.follower(&stats);
    follower.pass().unwrap();
    assert_eq!(
        tool_status(&stats),
        serde_json::json!({ "turns": 0, "skipped": 0, "scanning": false, "not_installed": true })
    );
    fs::remove_file(s.home.join(".fake-uninstalled")).unwrap();
    follower.pass().unwrap();
    assert_eq!(tool_status(&stats).get("not_installed"), None);
}

#[test]
fn turns_and_skipped_lines_are_counted() {
    let s = Setup::new("counts");
    append(&s.log("a.log"), "one\nbad\n# nothing\ntwo\nbad\n");
    let stats = Stats::default();
    s.follower(&stats).pass().unwrap();
    assert_eq!(
        without_last_turn(tool_status(&stats)),
        serde_json::json!({ "turns": 2, "skipped": 2, "scanning": false })
    );
}

#[test]
fn a_stored_turn_tells_when_and_a_tool_with_none_tells_nothing() {
    let s = Setup::new("last-turn");
    let stats = Stats::default();
    let started = SystemTime::now();
    // A pass that reads only a skipped line and a mark stores no turn.
    append(&s.log("a.log"), "bad\n# nothing\n");
    s.follower(&stats).pass().unwrap();
    let before = tool_status(&stats);
    assert!(before.get("last_turn").is_none());
    assert_eq!(before["turns"], 0);
    assert_eq!(before["skipped"], 1);

    append(&s.log("a.log"), "one\n");
    s.follower(&stats).pass().unwrap();
    let shown = tool_status(&stats);
    let text = shown["last_turn"].as_str().unwrap();
    assert!(text.ends_with('Z'), "UTC: {text}");
    let at = parse_time(text);
    assert!(at >= started, "{text} is before the start of the test");
    assert!(at <= SystemTime::now());
    assert_eq!(
        without_last_turn(shown),
        serde_json::json!({ "turns": 1, "skipped": 1, "scanning": false })
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
    append(&s.log("a.log"), "secret\n");
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

    let user = claude_user;
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
    append(
        &codex_log,
        &format!(
            "{}\n{{broken\n{}\n{}\n",
            codex_meta("01a0bee2-6260-73a3-b389-43c86bb8930f"),
            codex_user("fix it"),
            codex_item("event_msg", r#"{"type":"token_count"}"#),
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
        without_last_turn(all["claude-code"].clone()),
        serde_json::json!({ "turns": 2, "skipped": 2, "scanning": false })
    );
    assert_eq!(
        without_last_turn(all["codex"].clone()),
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

/// The minutes (`2026-11-11T10:15`) of the marks and of the turns stored for a session.
fn stored_minutes(s: &Setup, tool: &str, session: &str) -> (Vec<String>, Vec<String>) {
    let text = fs::read_to_string(s.data.join(format!("records/{tool}/{session}.jsonl"))).unwrap();
    let (mut marks, mut turns) = (Vec::new(), Vec::new());
    for line in text.lines() {
        let record: serde_json::Value = serde_json::from_str(line).unwrap();
        let minute = record["time"].as_str().unwrap()[..16].to_owned();
        match record["kind"].as_str().unwrap() {
            "mark" => marks.push(minute),
            _ => turns.push(minute),
        }
    }
    (marks, turns)
}

fn real_registry() -> Registry {
    Registry::new(vec![
        Box::new(Under(ClaudeCode, ".claude/projects")),
        Box::new(Under(Codex, ".codex/sessions")),
    ])
}

#[test]
fn a_claude_code_minute_with_only_a_prompt_or_a_reply_has_one_mark() {
    let s = Setup::new("claude-turn-marks");
    let log = s.home.join(".claude/projects/project/sess-1.jsonl");
    fs::create_dir_all(log.parent().unwrap()).unwrap();
    let at = |minute: &str, line: String| line.replace("10:15:30", minute);
    let reply = r#"{"type":"assistant","timestamp":"2026-11-11T10:17:05Z","cwd":"/w","sessionId":"sess-1","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}}"#;
    append(
        &log,
        &format!(
            "{}\n{reply}\n{}\n",
            at("10:15:30", claude_user("one")),
            at("10:16:10", claude_user("two"))
        ),
    );
    let stats = Stats::default();
    s.follower_of(real_registry(), &stats).pass().unwrap();
    let (marks, turns) = stored_minutes(&s, "claude-code", "sess-1");
    assert_eq!(turns.len(), 3);
    assert_eq!(
        marks,
        ["2026-11-11T10:15", "2026-11-11T10:17", "2026-11-11T10:16"]
    );
    // A restart replays nothing twice, marks included.
    s.follower_of(real_registry(), &stats).pass().unwrap();
    assert_eq!(stored_minutes(&s, "claude-code", "sess-1"), (marks, turns));
}

#[test]
fn a_codex_minute_with_only_a_prompt_or_a_reply_has_one_mark() {
    let s = Setup::new("codex-turn-marks");
    let id = "01a0bee2-6260-73a3-b389-43c86bb8930f";
    let log = s.home.join(format!(
        ".codex/sessions/2026/11/11/rollout-2026-11-11T12-00-00-{id}.jsonl"
    ));
    fs::create_dir_all(log.parent().unwrap()).unwrap();
    let at = |time: &str, line: String| line.replace("10:15:30", time);
    let reply = codex_item(
        "response_item",
        r#"{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}"#,
    );
    append(
        &log,
        &format!(
            "{}\n{}\n{}\n",
            codex_meta(id),
            at("12:05:03", codex_user("fix it")),
            at("12:07:03", reply)
        ),
    );
    let stats = Stats::default();
    s.follower_of(real_registry(), &stats).pass().unwrap();
    let (marks, turns) = stored_minutes(&s, "codex", id);
    assert_eq!(turns, ["2026-11-11T12:05", "2026-11-11T12:07"]);
    assert_eq!(marks, turns);
    s.follower_of(real_registry(), &stats).pass().unwrap();
    assert_eq!(stored_minutes(&s, "codex", id), (marks, turns));
}

/// A line of a subagent that works in the session `sess-1` at `time` (`10:15:30`), with
/// text that must never be stored.
fn subagent_line(time: &str) -> String {
    format!(
        r#"{{"type":"assistant","isSidechain":true,"timestamp":"2026-11-11T{time}Z","cwd":"/w","sessionId":"sess-1","message":{{"role":"assistant","content":[{{"type":"text","text":"subagent secret"}}]}}}}"#
    )
}

/// One subagent line for each of the 20 minutes from 10:01 to 10:20.
fn subagent_run() -> String {
    (1..=20)
        .map(|m| subagent_line(&format!("10:{m:02}:30")) + "\n")
        .collect()
}

/// The parent prompts at 10:00 and ends at 10:21, so a minute the subagent covers is the
/// only thing between them.
fn parent_around_a_run(log: &Path, middle: &str) {
    let at = |time: &str| claude_user("go").replace("10:15:30", time);
    append(
        log,
        &format!("{}\n{middle}{}\n", at("10:00:30"), at("10:21:30")),
    );
}

fn assert_no_hole(s: &Setup) {
    let (marks, turns) = stored_minutes(s, "claude-code", "sess-1");
    let all: Vec<String> = (0..=21).map(|m| format!("2026-11-11T10:{m:02}")).collect();
    let mut sorted = marks.clone();
    sorted.sort();
    assert_eq!(sorted, all, "one mark for each minute, none missing");
    assert_eq!(turns.len(), 2, "only the parent's prompts are turns");
    let stored = fs::read_to_string(s.data.join("records/claude-code/sess-1.jsonl")).unwrap();
    assert!(!stored.contains("subagent secret"));
}

#[test]
fn a_subagent_run_in_the_parent_file_leaves_no_hole_in_the_parent_minutes() {
    let s = Setup::new("subagent-same-file");
    let log = s.home.join(".claude/projects/project/sess-1.jsonl");
    fs::create_dir_all(log.parent().unwrap()).unwrap();
    parent_around_a_run(&log, &subagent_run());
    let stats = Stats::default();
    s.follower_of(real_registry(), &stats).pass().unwrap();
    assert_no_hole(&s);
    // A restart replays nothing twice.
    s.follower_of(real_registry(), &stats).pass().unwrap();
    assert_no_hole(&s);
}

#[test]
fn a_subagent_run_in_files_of_its_own_leaves_no_hole_in_the_parent_minutes() {
    let s = Setup::new("subagent-own-file");
    let project = s.home.join(".claude/projects/project");
    let log = project.join("sess-1.jsonl");
    let side = project.join("sess-1/subagents/agent-a1.jsonl");
    let flow = project.join("sess-1/subagents/workflows/wf_1");
    fs::create_dir_all(&flow).unwrap();
    parent_around_a_run(&log, "");
    // The lines name no session: only the folder says whose they are. The second half of
    // the run is a workflow's agent, one folder deeper.
    let own = |range: std::ops::RangeInclusive<u32>| -> String {
        range
            .map(|m| {
                subagent_line(&format!("10:{m:02}:30")).replace(r#""sessionId":"sess-1","#, "")
                    + "\n"
            })
            .collect()
    };
    append(&side, &own(1..=10));
    append(&flow.join("agent-b1.jsonl"), &own(11..=20));
    // A workflow's journal is no agent's log: its minute must not become a mark.
    append(
        &flow.join("journal.jsonl"),
        &(subagent_line("10:50:30") + "\n"),
    );
    let stats = Stats::default();
    s.follower_of(real_registry(), &stats).pass().unwrap();
    assert_no_hole(&s);
    s.follower_of(real_registry(), &stats).pass().unwrap();
    assert_no_hole(&s);
    // The subagent files add no session of their own.
    assert_eq!(
        fs::read_dir(s.data.join("records/claude-code"))
            .unwrap()
            .count(),
        1
    );
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
        without_last_turn(tool_status(&stats)),
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

    fn forget(&mut self) {
        self.inner.forget();
    }
}

#[test]
fn a_crash_in_a_long_first_scan_keeps_what_the_checkpoint_saved() {
    let s = Setup::new("checkpoint");
    let log = s.log("a.log");
    // 10 bytes a line. The checkpoint comes after 5000 lines, or earlier when the pass is slow
    // enough for the timer to spend the one save: the test must hold either way.
    let lines = (0..6000)
        .map(|i| format!("line{i:05}\n"))
        .collect::<Vec<_>>()
        .concat();
    append(&log, &lines);
    let sink = Crashing {
        inner: FileSink::new(&s.data),
        // Two appends a line: its mark, then its turn.
        appends: 2 * 5200,
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
    let offset = offset.expect("the checkpoint saved nothing");
    assert_eq!(offset % 10, 0, "the checkpoint cuts a line: {offset}");
    assert!(
        offset <= 5200 * 10,
        "the checkpoint is past the crash: {offset}"
    );

    // The pass stops at its first failing save or append, which a slow pass reaches long before
    // line 5200: how many turns were stored by then is not fixed.
    let stored = s.texts_up_to("a", usize::MAX);
    assert!(
        stored.len() <= 5200,
        "{} turns stored before the crash",
        stored.len()
    );

    let stats = Stats::default();
    s.follower(&stats).pass().unwrap();
    // Only what the crash left unstored is new; what was stored already is a replay.
    assert_eq!(tool_status(&stats)["turns"], 6000 - stored.len());
    // Every line is stored once, in order: no gap and no duplicate.
    let all = s.texts_up_to("a", usize::MAX);
    assert_eq!(all.len(), 6000);
    assert!(
        all.iter()
            .enumerate()
            .all(|(i, text)| *text == format!("line{i:05}")),
        "the stored turns are not the 6000 lines in order"
    );
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
    // On until the first file has been stored: the user turns collection off during the scan.
    let stored = s.data.join("records/fake/a.jsonl");
    let switch: follow::Switch = Arc::new(move |_| !stored.exists());
    let stats = Stats::default();
    s.follower(&stats).with_switch(switch).pass().unwrap();
    assert_eq!(s.texts("a"), ["one"]);
    assert!(s.texts("b").is_empty());
}

/// The real tools under a temporary home, with the real settings as the switch: what the
/// agent does with the user's three collection switches.
struct Switched {
    s: Setup,
    settings: construct_agent::settings::Settings,
    claude: PathBuf,
    codex: PathBuf,
}

const CODEX_SESSION: &str = "01a0bee2-6260-73a3-b389-43c86bb8930f";
const NEW_SESSION: &str = "01a0bee2-6260-73a3-b389-43c86bb8930e";

impl Switched {
    fn new(name: &str) -> Self {
        let s = Setup::new(name);
        let claude = s.home.join(".claude/projects/project/sess-1.jsonl");
        let codex = s.home.join(format!(
            ".codex/sessions/2026/11/11/rollout-2026-11-11T10-15-00-{CODEX_SESSION}.jsonl"
        ));
        fs::create_dir_all(claude.parent().unwrap()).unwrap();
        fs::create_dir_all(codex.parent().unwrap()).unwrap();
        let settings = construct_agent::settings::Settings::open(&s.root.join("config")).unwrap();
        append(&codex, &format!("{}\n", codex_meta(CODEX_SESSION)));
        Self {
            s,
            settings,
            claude,
            codex,
        }
    }

    fn say_claude(&self, text: &str) {
        append(&self.claude, &format!("{}\n", claude_user(text)));
    }

    fn say_codex(&self, text: &str) {
        append(&self.codex, &format!("{}\n", codex_user(text)));
    }

    /// A follower as an agent start makes it, with the settings as its switch.
    fn follower(&self, stats: &Stats) -> Follower<FileSink> {
        let registry = Registry::new(vec![
            Box::new(Under(ClaudeCode, ".claude/projects")),
            Box::new(Under(Codex, ".codex/sessions")),
        ]);
        let settings = self.settings.clone();
        self.s
            .follower_of(registry, stats)
            .with_switch(Arc::new(move |tool| settings.collects(tool)))
    }

    fn set(&self, json: &str) {
        self.settings
            .apply(construct_agent::settings::Update::parse(json).unwrap())
            .unwrap();
    }

    fn stored(&self, tool: &str, session: &str) -> Vec<String> {
        FileSink::new(&self.s.data)
            .turns_before(tool, session, "9999", 1000)
            .unwrap()
            .into_iter()
            .map(|t| t.text)
            .collect()
    }
}

#[test]
fn a_tool_that_is_off_stores_nothing_and_a_tool_that_is_on_is_not_affected() {
    let t = Switched::new("tool-off");
    t.set(r#"{"collection":{"codex":false}}"#);
    let stats = Stats::default();
    let mut follower = t.follower(&stats);
    t.say_claude("c1");
    t.say_codex("x1");
    follower.pass().unwrap();
    t.say_claude("c2");
    t.say_codex("x2");
    follower.pass().unwrap();
    assert_eq!(t.stored("claude-code", "sess-1"), ["c1", "c2"]);
    assert!(t.stored("codex", CODEX_SESSION).is_empty());
    let shown = stats.to_json();
    assert_eq!(shown["codex"]["off"], true);
    assert_eq!(shown["codex"]["turns"], 0);
    assert!(shown["claude-code"].get("off").is_none(), "{shown}");

    // Switched on: what was written while it was off is never collected, what comes next is.
    t.set(r#"{"collection":{"codex":true}}"#);
    follower.pass().unwrap();
    assert!(t.stored("codex", CODEX_SESSION).is_empty());
    t.say_codex("x3");
    follower.pass().unwrap();
    assert_eq!(t.stored("codex", CODEX_SESSION), ["x3"]);
    assert!(stats.to_json()["codex"].get("off").is_none());

    // The other tool is switched off in its turn; Codex goes on.
    t.set(r#"{"collection":{"claude_code":false}}"#);
    t.say_claude("c3");
    t.say_codex("x4");
    follower.pass().unwrap();
    assert_eq!(t.stored("claude-code", "sess-1"), ["c1", "c2"]);
    assert_eq!(t.stored("codex", CODEX_SESSION), ["x3", "x4"]);
    assert_eq!(stats.to_json()["claude-code"]["off"], true);
}

#[test]
fn the_master_switch_stops_both_tools_and_turning_it_on_collects_only_what_comes_next() {
    let t = Switched::new("master-off");
    let stats = Stats::default();
    let mut follower = t.follower(&stats);
    t.say_claude("c1");
    t.say_codex("x1");
    follower.pass().unwrap();

    t.set(r#"{"collection":{"enabled":false}}"#);
    t.say_claude("c2");
    t.say_codex("x2");
    follower.pass().unwrap();
    // The records collected before stay; nothing new is stored.
    assert_eq!(t.stored("claude-code", "sess-1"), ["c1"]);
    assert_eq!(t.stored("codex", CODEX_SESSION), ["x1"]);
    let shown = stats.to_json();
    assert_eq!(shown["claude-code"]["off"], true);
    assert_eq!(shown["codex"]["off"], true);

    // Master on, but a tool's own switch is off: that tool stays off.
    t.set(r#"{"collection":{"enabled":true,"codex":false}}"#);
    t.say_claude("c3");
    t.say_codex("x3");
    follower.pass().unwrap();
    assert_eq!(t.stored("claude-code", "sess-1"), ["c1", "c3"]);
    assert_eq!(t.stored("codex", CODEX_SESSION), ["x1"]);
}

#[test]
fn a_fresh_agent_with_a_tool_off_reads_nothing_of_it_and_a_session_that_appears_is_skipped() {
    let t = Switched::new("fresh");
    t.say_claude("c1");
    t.say_codex("x1");
    t.set(r#"{"collection":{"codex":false}}"#);

    // The very first scan of an agent that has never run.
    let stats = Stats::default();
    t.follower(&stats).pass().unwrap();
    assert_eq!(t.stored("claude-code", "sess-1"), ["c1"]);
    assert!(t.stored("codex", CODEX_SESSION).is_empty());
    assert!(!t.s.data.join("records/codex").exists());
    assert_eq!(stats.to_json()["codex"]["scanning"], false);
    assert_eq!(stats.to_json()["codex"]["off"], true);

    // A new session of the tool appears while it is off, and the agent is restarted.
    let new = t
        .codex
        .with_file_name("rollout-2026-11-11T11-00-00-01a0bee2-6260-73a3-b389-43c86bb8930e.jsonl");
    append(
        &new,
        &format!(
            "{}
",
            codex_meta(NEW_SESSION)
        ),
    );
    // It already holds a user message: a file read from its start would store it.
    append(
        &new,
        &format!(
            "{}
",
            codex_user("n0")
        ),
    );
    t.say_codex("x2");
    let mut follower = t.follower(&Stats::default());
    follower.pass().unwrap();

    // Switched on: nothing of the time it was off, in the old file or in the new one.
    t.set(r#"{"collection":{"codex":true}}"#);
    follower.pass().unwrap();
    assert!(t.stored("codex", CODEX_SESSION).is_empty());
    t.say_codex("x3");
    append(
        &new,
        &format!(
            "{}
",
            codex_user("n1")
        ),
    );
    follower.pass().unwrap();
    assert_eq!(t.stored("codex", CODEX_SESSION), ["x3"]);
    assert_eq!(t.stored("codex", NEW_SESSION), ["n1"]);
}
