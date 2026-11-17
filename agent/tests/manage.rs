//! Prompts to the Construct host: the live queue, the backfill, the journal and the retries,
//! against a fake host (a local port that records the calls and answers as scripted).
#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use connectors::{Record, Role, Turn};
use construct_agent::chat::Link;
use construct_agent::host::Connection;
use construct_agent::manage::{CONTEXT_TURNS, Manager, Timing};
use construct_agent::records::{FileSink, Progress, Sink};
use construct_agent::settings::{Settings, Update};
use serde_json::{Value, json};

mod common;

const TOKEN: &str = "manage-token-77c1";
const STORED: &str = r#"{"outcome":"stored","reason":null,"added":1,"replaced":0,"removed":0}"#;
const NO_CHANGE: &str =
    r#"{"outcome":"nothing_changed","reason":"no_change","added":0,"replaced":0,"removed":0}"#;

struct Call {
    path: String,
    authorization: String,
    body: Value,
}

/// A host that answers every call with the next scripted answer, then with `STORED`.
struct FakeHost {
    addr: SocketAddr,
    calls: Arc<Mutex<Vec<Call>>>,
    script: Arc<Mutex<VecDeque<(u16, String)>>>,
    delay: Arc<Mutex<Duration>>,
}

impl FakeHost {
    fn start() -> Self {
        Self::start_at(0)
    }

    fn start_at(port: u16) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
        let addr = listener.local_addr().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let script = Arc::new(Mutex::new(VecDeque::new()));
        let delay = Arc::new(Mutex::new(Duration::ZERO));
        let (c, s, d) = (Arc::clone(&calls), Arc::clone(&script), Arc::clone(&delay));
        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let call = read_call(&mut stream);
                c.lock().unwrap().push(call);
                thread::sleep(*d.lock().unwrap());
                let (code, body) = s
                    .lock()
                    .unwrap()
                    .pop_front()
                    .unwrap_or((200, STORED.to_string()));
                common::status(&mut stream, code, &body);
            }
        });
        Self {
            addr,
            calls,
            script,
            delay,
        }
    }

    fn connection(&self) -> Connection {
        Connection {
            port: self.addr.port(),
            token: TOKEN.into(),
        }
    }

    fn answer(&self, code: u16, body: &str) {
        self.script.lock().unwrap().push_back((code, body.into()));
    }

    fn count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    /// The `message` of every call so far.
    fn messages(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|c| c.body["message"].as_str().unwrap().to_string())
            .collect()
    }
}

fn read_call(stream: &mut TcpStream) -> Call {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    reader.read_line(&mut request_line).unwrap();
    let (mut length, mut authorization) = (0, String::new());
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let lower = line.to_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            length = v.trim().parse().unwrap();
        }
        if lower.starts_with("authorization:") {
            authorization = line.split_once(':').unwrap().1.trim().to_string();
        }
        if line.trim().is_empty() {
            break;
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let path = request_line.split_whitespace().nth(1).unwrap().to_string();
    assert!(request_line.starts_with("POST "), "{request_line}");
    Call {
        path,
        authorization,
        body: serde_json::from_slice(&body).unwrap(),
    }
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Stand {
    dir: PathBuf,
    settings: Settings,
    link: Link,
}

impl Stand {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "cb-manage-it-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let settings = Settings::open(&dir.join("config")).unwrap();
        let update =
            json!({ "model": { "base_url": "http://127.0.0.1:9", "model": "m", "api_key": "k" } });
        settings
            .apply(Update::parse(&update.to_string()).unwrap())
            .unwrap();
        Self {
            dir: dir.join("data"),
            settings,
            link: Link::default(),
        }
    }

    fn manager(&self) -> Manager {
        let timing = Timing {
            retry: [Duration::from_millis(10); 2],
            idle: Duration::from_millis(20),
            failed: Duration::from_millis(20),
            refused: Duration::from_millis(50),
            call: Duration::from_millis(600),
        };
        Manager::open_with(&self.dir, self.settings.clone(), self.link.clone(), timing).unwrap()
    }

    fn manager_with_failed_pause(&self, failed: Duration) -> Manager {
        let timing = Timing {
            retry: [Duration::from_millis(10); 2],
            idle: Duration::from_millis(20),
            failed,
            refused: Duration::from_millis(50),
            call: Duration::from_millis(600),
        };
        Manager::open_with(&self.dir, self.settings.clone(), self.link.clone(), timing).unwrap()
    }

    fn data(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        if let Some(parent) = self.dir.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }
}

fn turn(tool: &str, session: &str, time: &str, role: Role, text: &str) -> Turn {
    Turn {
        tool: tool.into(),
        session: session.into(),
        time: time.into(),
        folder: String::new(),
        branch: String::new(),
        role,
        text: text.into(),
    }
}

/// Stores turns the way the readers do.
fn store(data: &Path, turns: &[Turn]) {
    let mut sink = FileSink::new(data);
    for t in turns {
        sink.append(&Record::Turn(t.clone())).unwrap();
    }
    sink.save_progress(&Progress::default()).unwrap();
}

/// A time a little after the first start (the first start is when the manager opens).
fn later(seconds: u64) -> String {
    let now = std::time::SystemTime::now() + Duration::from_secs(seconds);
    let t = time::OffsetDateTime::from(now);
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

/// `n` ago, as a time in the record store: before the manager's first start.
fn ago(days: i64, minute: u32) -> String {
    let t = time::OffsetDateTime::now_utc() - time::Duration::days(days)
        + time::Duration::minutes(i64::from(minute));
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(20));
    }
}

fn backfill_state(manage: &Manager) -> String {
    manage.status()["backfill"]["state"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Gives `n` history prompts to the store: `a` of them from codex and the rest from claude-code.
fn history(data: &Path, n: usize, a: usize) {
    for i in 0..n {
        let (tool, session) = if i < a {
            ("codex", "cx")
        } else {
            ("claude-code", "cc")
        };
        let minute = u32::try_from(i).unwrap();
        store(
            data,
            &[
                turn(
                    tool,
                    session,
                    &ago(3, minute * 2),
                    Role::User,
                    &format!("prompt {i:03}"),
                ),
                turn(
                    tool,
                    session,
                    &ago(3, minute * 2 + 1),
                    Role::Assistant,
                    "ok",
                ),
            ],
        );
    }
}

#[test]
fn a_live_prompt_goes_out_with_the_turns_before_it_as_context() {
    let stand = Stand::new();
    let host = FakeHost::start();
    // Twelve turns before the prompt: only the last ten go with it.
    let mut before = Vec::new();
    for i in 0..12 {
        let role = if i % 2 == 0 {
            Role::User
        } else {
            Role::Assistant
        };
        before.push(turn(
            "claude-code",
            "s1",
            &ago(1, u32::try_from(i).unwrap()),
            role,
            &format!("turn {i}"),
        ));
    }
    let manage = stand.manager();
    let prompt = turn(
        "claude-code",
        "s1",
        &later(60),
        Role::User,
        "I write Rust in the gears-rust repository",
    );
    before.push(prompt.clone());
    store(stand.data(), &before);
    stand.link.set(host.connection());
    manage.spawn();
    (manage.observer())(&prompt);

    wait_for("the call", || host.count() == 1);
    let call = &host.calls.lock().unwrap()[0];
    assert!(call.path.ends_with("/facts/manage"), "{}", call.path);
    assert!(
        call.path.starts_with("/construct/v1/subjects/"),
        "{}",
        call.path
    );
    assert_eq!(call.authorization, format!("Bearer {TOKEN}"));
    assert_eq!(
        call.body["message"],
        "I write Rust in the gears-rust repository"
    );
    let conversation = call.body["conversation"].as_str().unwrap();
    assert_eq!(conversation.lines().count(), CONTEXT_TURNS);
    assert!(conversation.starts_with("user: turn 2\n"), "{conversation}");
    assert!(
        conversation.ends_with("assistant: turn 11"),
        "{conversation}"
    );
    wait_for("the status", || manage.status()["live"]["stored"] == 1);
    assert_eq!(manage.status()["added"], 1);
}

#[test]
fn a_prompt_from_before_the_first_start_is_not_sent_by_itself() {
    let stand = Stand::new();
    let host = FakeHost::start();
    let old = turn("codex", "c1", &ago(2, 0), Role::User, "an old prompt");
    store(stand.data(), std::slice::from_ref(&old));
    let manage = stand.manager();
    stand.link.set(host.connection());
    manage.spawn();
    (manage.observer())(&old);
    // A new one proves the thread is awake.
    let new = turn("codex", "c1", &later(30), Role::User, "a new prompt");
    store(stand.data(), std::slice::from_ref(&new));
    (manage.observer())(&new);

    wait_for("the new prompt", || host.count() >= 1);
    thread::sleep(Duration::from_millis(200));
    assert_eq!(host.messages(), ["a new prompt"]);
}

#[test]
fn a_dry_run_counts_the_prompts_per_tool_and_sends_nothing() {
    let stand = Stand::new();
    let host = FakeHost::start();
    history(stand.data(), 7, 3);
    let manage = stand.manager();
    stand.link.set(host.connection());
    manage.spawn();

    let reply: Value = serde_json::from_str(&manage.backfill(r#"{"dry_run":true}"#)).unwrap();
    assert_eq!(reply["dry_run"], true);
    assert_eq!(reply["prompts"], json!({ "claude-code": 4, "codex": 3 }));
    assert_eq!(reply["total"], 7);
    let limited: Value =
        serde_json::from_str(&manage.backfill(r#"{"dry_run":true,"limit":2}"#)).unwrap();
    assert_eq!(limited["would_send"], 2);
    // A window shorter than the age of the prompts leaves them out.
    let short: Value =
        serde_json::from_str(&manage.backfill(r#"{"dry_run":true,"days":1}"#)).unwrap();
    assert_eq!(short["total"], 0);
    thread::sleep(Duration::from_millis(100));
    assert_eq!(host.count(), 0);
}

#[test]
fn a_limited_backfill_sends_that_many_oldest_first_and_the_next_run_the_rest() {
    let stand = Stand::new();
    let host = FakeHost::start();
    history(stand.data(), 25, 10);
    let manage = stand.manager();
    stand.link.set(host.connection());
    manage.spawn();

    let reply: Value = serde_json::from_str(&manage.backfill(r#"{"limit":20}"#)).unwrap();
    assert_eq!(reply["queued"], 20, "{reply}");
    wait_for("the first run", || backfill_state(&manage) == "done");
    assert_eq!(host.count(), 20);
    // Oldest first, across both tools.
    let first = host.messages();
    let expected: Vec<String> = (0..20).map(|i| format!("prompt {i:03}")).collect();
    assert_eq!(first, expected);

    let reply: Value = serde_json::from_str(&manage.backfill("{}")).unwrap();
    assert_eq!(reply["queued"], 5, "{reply}");
    wait_for("the second run", || host.count() == 25);
    wait_for("the second run to end", || {
        backfill_state(&manage) == "done"
    });
    let all = host.messages();
    let mut sorted = all.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 25, "a prompt was sent twice: {all:?}");
    assert_eq!(
        all[20..],
        (20..25)
            .map(|i| format!("prompt {i:03}"))
            .collect::<Vec<_>>()
    );
    let status = manage.status();
    assert_eq!(status["sent"], 25);
    assert_eq!(status["backfill"]["sent"], 5);
}

#[test]
fn a_restart_does_not_send_a_prompt_again() {
    let stand = Stand::new();
    let host = FakeHost::start();
    history(stand.data(), 6, 0);
    {
        let manage = stand.manager();
        stand.link.set(host.connection());
        manage.spawn();
        let _ = manage.backfill(r#"{"limit":4}"#);
        wait_for("four calls", || backfill_state(&manage) == "done");
    }
    assert_eq!(host.count(), 4);
    // The agent starts again on the same files: the first start is still the old one.
    let manage = stand.manager();
    manage.spawn();
    let dry: Value = serde_json::from_str(&manage.backfill(r#"{"dry_run":true}"#)).unwrap();
    assert_eq!(dry["total"], 2, "{dry}");
    let _ = manage.backfill("{}");
    wait_for("the rest", || host.count() == 6);
    wait_for("the end", || backfill_state(&manage) == "done");
    let mut all = host.messages();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 6);
}

#[test]
fn live_prompts_that_were_stored_but_not_sent_go_out_after_a_restart() {
    let stand = Stand::new();
    let host = FakeHost::start();
    let first = stand.manager();
    let prompt = turn(
        "claude-code",
        "s",
        &later(5),
        Role::User,
        "typed while the agent was down",
    );
    store(stand.data(), std::slice::from_ref(&prompt));
    drop(first);
    stand.link.set(host.connection());
    let manage = stand.manager();
    manage.spawn();
    wait_for("the recovered prompt", || host.count() == 1);
    assert_eq!(host.messages(), ["typed while the agent was down"]);
}

#[test]
fn a_live_prompt_waits_for_one_call_at_most_while_a_backfill_runs() {
    let stand = Stand::new();
    let host = FakeHost::start();
    history(stand.data(), 8, 0);
    *host.delay.lock().unwrap() = Duration::from_millis(150);
    let manage = stand.manager();
    stand.link.set(host.connection());
    manage.spawn();
    let _ = manage.backfill("{}");
    wait_for("the backfill to start", || host.count() >= 1);
    let live = turn("claude-code", "cc", &later(30), Role::User, "live one");
    store(stand.data(), std::slice::from_ref(&live));
    (manage.observer())(&live);
    wait_for("the end", || backfill_state(&manage) == "done");
    let order = host.messages();
    let at = order.iter().position(|m| m == "live one").unwrap();
    assert!(at <= 2, "the live prompt was call {at} of {}", order.len());
}

#[test]
fn an_unavailable_answer_is_retried_and_the_prompt_is_sent_once_in_the_end() {
    let stand = Stand::new();
    let host = FakeHost::start();
    host.answer(503, r#"{"title":"unavailable"}"#);
    host.answer(
        200,
        r#"{"outcome":"nothing_changed","reason":"timeout","added":0,"replaced":0,"removed":0}"#,
    );
    let manage = stand.manager();
    stand.link.set(host.connection());
    manage.spawn();
    let prompt = turn("codex", "c", &later(5), Role::User, "retry me");
    store(stand.data(), std::slice::from_ref(&prompt));
    (manage.observer())(&prompt);
    wait_for("the third call", || host.count() == 3);
    wait_for("the status", || manage.status()["live"]["stored"] == 1);
    assert_eq!(host.messages(), ["retry me", "retry me", "retry me"]);
    thread::sleep(Duration::from_millis(150));
    assert_eq!(host.count(), 3, "a prompt that was stored is sent again");
}

#[test]
fn a_host_that_refuses_the_agent_stops_the_backfill_with_the_reason() {
    let stand = Stand::new();
    let host = FakeHost::start();
    history(stand.data(), 5, 0);
    host.answer(404, r#"{"title":"not found"}"#);
    let manage = stand.manager();
    stand.link.set(host.connection());
    manage.spawn();
    let _ = manage.backfill("{}");
    wait_for("the stop", || backfill_state(&manage) == "stopped");
    let status = manage.status();
    assert_eq!(status["backfill"]["error"], "the host answered 404");
    assert_eq!(host.count(), 1);
    // The prompt was taken back: the next run sends it.
    let dry: Value = serde_json::from_str(&manage.backfill(r#"{"dry_run":true}"#)).unwrap();
    assert_eq!(dry["total"], 5);
}

#[test]
fn a_backfill_stops_after_prompts_fail_one_after_the_other() {
    let stand = Stand::new();
    let host = FakeHost::start();
    history(stand.data(), 10, 0);
    for _ in 0..9 {
        host.answer(503, "{}");
    }
    let manage = stand.manager();
    stand.link.set(host.connection());
    manage.spawn();
    let _ = manage.backfill("{}");
    wait_for("the stop", || backfill_state(&manage) == "stopped");
    assert_eq!(host.count(), 9);
    assert_eq!(manage.status()["backfill"]["failed"], 3);
}

#[test]
fn a_backfill_is_refused_without_a_host_a_model_or_while_one_runs() {
    let stand = Stand::new();
    let host = FakeHost::start();
    history(stand.data(), 3, 0);
    let manage = stand.manager();
    let reply = manage.backfill("{}");
    assert!(reply.contains("not running"), "{reply}");
    stand.link.set(host.connection());
    stand
        .settings
        .apply(Update::parse(r#"{"model":{"remove_key":true}}"#).unwrap())
        .unwrap();
    let reply = manage.backfill("{}");
    assert!(reply.contains("no model"), "{reply}");
    assert!(manage.backfill(r#"{"days":0}"#).contains("error"));
    assert!(manage.backfill(r#"{"nope":1}"#).contains("error"));
    // The dry run needs neither.
    assert!(
        manage
            .backfill(r#"{"dry_run":true}"#)
            .contains(r#""total":3"#)
    );
}

#[test]
fn nothing_a_prompt_said_is_in_the_status_or_the_journal() {
    let stand = Stand::new();
    let host = FakeHost::start();
    history(stand.data(), 2, 0);
    let manage = stand.manager();
    stand.link.set(host.connection());
    manage.spawn();
    let _ = manage.backfill("{}");
    wait_for("the end", || backfill_state(&manage) == "done");
    assert!(!manage.status().to_string().contains("prompt 00"));
    let journal = std::fs::read_to_string(stand.data().join("manage-sent.log")).unwrap();
    assert!(!journal.contains("prompt"));
    for file in ["manage-sent.log", "manage-state.json"] {
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(stand.data().join(file))
                .unwrap()
                .permissions(),
        );
        assert_eq!(mode & 0o777, 0o600, "{file}");
    }
}

#[test]
fn the_text_of_the_no_change_answer_counts_as_sent() {
    let stand = Stand::new();
    let host = FakeHost::start();
    host.answer(200, NO_CHANGE);
    let manage = stand.manager();
    stand.link.set(host.connection());
    manage.spawn();
    let prompt = turn("codex", "c", &later(5), Role::User, "thanks");
    store(stand.data(), std::slice::from_ref(&prompt));
    (manage.observer())(&prompt);
    wait_for("the status", || manage.status()["live"]["sent"] == 1);
    assert_eq!(manage.status()["live"]["stored"], 0);
}

/// How many prompts the journal holds as sent.
fn journal_marks(data: &Path) -> usize {
    let text = std::fs::read_to_string(data.join("manage-sent.log")).unwrap_or_default();
    let mut sent = std::collections::HashSet::new();
    for line in text.lines() {
        match line.split_at(1) {
            ("+", key) => sent.insert(key.to_string()),
            _ => sent.remove(&line[1..]),
        };
    }
    sent.len()
}

fn live_prompt(stand: &Stand, manage: &Manager, text: &str, seconds: u64) {
    let prompt = turn("codex", "c", &later(seconds), Role::User, text);
    store(stand.data(), std::slice::from_ref(&prompt));
    (manage.observer())(&prompt);
}

#[test]
fn a_write_whose_result_is_unknown_is_not_sent_again() {
    let stand = Stand::new();
    let host = FakeHost::start();
    host.answer(
        503,
        r#"{"detail":"The write was sent, but its result is unknown: it may have been stored."}"#,
    );
    let manage = stand.manager();
    stand.link.set(host.connection());
    manage.spawn();
    live_prompt(&stand, &manage, "maybe stored", 5);
    wait_for("the call", || host.count() == 1);
    wait_for("the queue to empty", || {
        manage.status()["live"]["queued"] == 0
    });
    live_prompt(&stand, &manage, "the next one", 6);
    wait_for("the next call", || host.count() == 2);
    assert_eq!(host.messages(), ["maybe stored", "the next one"]);
    assert_eq!(journal_marks(stand.data()), 2, "the mark was taken back");
}

#[test]
fn a_server_error_that_may_have_stored_the_prompt_is_not_sent_again() {
    let stand = Stand::new();
    let host = FakeHost::start();
    host.answer(502, "{}");
    let manage = stand.manager();
    stand.link.set(host.connection());
    manage.spawn();
    live_prompt(&stand, &manage, "behind a bad gateway", 5);
    wait_for("the queue to empty", || {
        host.count() == 1 && manage.status()["live"]["queued"] == 0
    });
    thread::sleep(Duration::from_millis(200));
    assert_eq!(host.count(), 1);
    assert_eq!(journal_marks(stand.data()), 1);
}

#[test]
fn a_refused_connection_leaves_the_prompt_unsent_and_it_goes_out_when_the_host_returns() {
    let stand = Stand::new();
    // A port nothing listens on.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    stand.link.set(Connection {
        port,
        token: TOKEN.into(),
    });
    let manage = stand.manager();
    manage.spawn();
    live_prompt(&stand, &manage, "typed while the host is down", 5);
    wait_for("the prompt to be left for the next start", || {
        manage.status()["live"]["failed"] == 1
    });
    assert_eq!(
        journal_marks(stand.data()),
        0,
        "an undelivered prompt stays marked"
    );
    assert_eq!(manage.status()["live"]["sent"], 0);

    let host = FakeHost::start_at(port);
    // The strikes ran out and left the prompt for the next start; a new manager takes it up.
    let again = stand.manager();
    again.spawn();
    wait_for("the call", || host.count() == 1);
    assert_eq!(host.messages(), ["typed while the host is down"]);
    wait_for("the status", || again.status()["live"]["sent"] == 1);
    assert_eq!(journal_marks(stand.data()), 1);
}

#[test]
fn a_stalled_host_does_not_hold_up_the_readers_or_status() {
    let stand = Stand::new();
    let host = FakeHost::start();
    *host.delay.lock().unwrap() = Duration::from_secs(3);
    history(stand.data(), 3, 0);
    let manage = stand.manager();
    stand.link.set(host.connection());
    manage.spawn();
    let _ = manage.backfill("{}");
    wait_for("a call in flight", || host.count() == 1);

    let started = Instant::now();
    let prompt = turn(
        "codex",
        "c",
        &later(5),
        Role::User,
        "typed during the stall",
    );
    store(stand.data(), std::slice::from_ref(&prompt));
    (manage.observer())(&prompt);
    let _ = manage.status();
    let dry = manage.backfill(r#"{"dry_run":true}"#);
    assert!(dry.contains("dry_run"), "{dry}");
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "the readers waited {:?} for a stalled call",
        started.elapsed()
    );
}

#[test]
fn a_refusal_of_one_prompt_drops_it_and_the_next_goes_out() {
    let stand = Stand::new();
    let host = FakeHost::start();
    host.answer(422, "{}");
    let manage = stand.manager();
    stand.link.set(host.connection());
    manage.spawn();
    live_prompt(&stand, &manage, "refused", 5);
    live_prompt(&stand, &manage, "accepted", 6);
    wait_for("both calls", || host.count() == 2);
    assert_eq!(host.messages(), ["refused", "accepted"]);
    wait_for("the status", || manage.status()["live"]["sent"] == 1);
    assert_eq!(manage.status()["live"]["failed"], 1);
}

#[test]
fn a_backoff_is_not_cut_short_by_a_new_prompt() {
    let stand = Stand::new();
    let host = FakeHost::start();
    for _ in 0..3 {
        host.answer(503, "{}");
    }
    let manage = stand.manager_with_failed_pause(Duration::from_millis(800));
    stand.link.set(host.connection());
    manage.spawn();
    live_prompt(&stand, &manage, "first", 5);
    // The first round of three failed calls is over, and the thread pauses.
    wait_for("the first round", || host.count() == 3);
    thread::sleep(Duration::from_millis(100));
    live_prompt(&stand, &manage, "second", 6);
    thread::sleep(Duration::from_millis(300));
    assert_eq!(host.count(), 3, "a new prompt cut the pause short");
    wait_for("the call after the pause", || host.count() >= 4);
}

#[test]
fn a_host_that_refuses_every_prompt_stops_the_backfill_after_a_few() {
    let stand = Stand::new();
    let host = FakeHost::start();
    history(stand.data(), 10, 0);
    for _ in 0..10 {
        host.answer(422, "{}");
    }
    let manage = stand.manager();
    stand.link.set(host.connection());
    manage.spawn();
    let _ = manage.backfill("{}");
    wait_for("the stop", || backfill_state(&manage) == "stopped");
    assert_eq!(host.count(), 3);
    assert_eq!(manage.status()["backfill"]["failed"], 3);
    assert_eq!(
        journal_marks(stand.data()),
        3,
        "only the refused prompts stay marked"
    );
}

#[test]
fn another_4xx_is_taken_for_a_refusal_of_every_call_and_marks_nothing() {
    let stand = Stand::new();
    let host = FakeHost::start();
    history(stand.data(), 5, 0);
    host.answer(405, "{}");
    let manage = stand.manager();
    stand.link.set(host.connection());
    manage.spawn();
    let _ = manage.backfill("{}");
    wait_for("the stop", || backfill_state(&manage) == "stopped");
    assert_eq!(host.count(), 1);
    assert_eq!(journal_marks(stand.data()), 0);
}

#[test]
fn a_call_that_timed_out_is_not_sent_again() {
    let stand = Stand::new();
    let host = FakeHost::start();
    // The host answers after the call has given up waiting (600 ms).
    *host.delay.lock().unwrap() = Duration::from_millis(1200);
    let manage = stand.manager();
    stand.link.set(host.connection());
    manage.spawn();
    live_prompt(&stand, &manage, "slow", 5);
    wait_for("the unsure count", || manage.status()["unsure"] == 1);
    thread::sleep(Duration::from_millis(300));
    assert_eq!(host.count(), 1, "a call that may have run was sent again");
    assert_eq!(journal_marks(stand.data()), 1);
    assert_eq!(manage.status()["live"]["queued"], 0);
}
