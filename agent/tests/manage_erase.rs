//! The erasure of the person and the switch that follows it, against a fake host (a local port
//! that records the calls and answers as scripted).
#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use connectors::{Author, Record, Role, Turn};
use construct_agent::chat::Link;
use construct_agent::host::Connection;
use construct_agent::manage::Manager;
use construct_agent::records::{FileSink, Progress, Sink};
use construct_agent::settings::{Settings, Update};
use serde_json::{Value, json};

mod common;

use common::fake_host::{FakeHost, TOKEN};
use common::stand::{set_collection, timing, wait_for};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const SETTINGS_ON: &str =
    r#"{"subject_id":"s","personalization_enabled":true,"erasure_in_progress":false}"#;
const SETTINGS_OFF: &str =
    r#"{"subject_id":"s","personalization_enabled":false,"erasure_in_progress":false}"#;
const ERASING: &str =
    r#"{"subject_id":"s","personalization_enabled":false,"erasure_in_progress":true}"#;

struct Stand {
    root: PathBuf,
    settings: Settings,
    link: Link,
    manage: Manager,
}

impl Stand {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "cb-erase-it-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        let settings = Settings::open(&root.join("config")).unwrap();
        // A model is set, so a queued prompt would really be sent.
        let model =
            json!({ "model": { "base_url": "http://127.0.0.1:9", "model": "m", "api_key": "k" } });
        settings
            .apply(Update::parse(&model.to_string()).unwrap())
            .unwrap();
        let link = Link::default();
        let manage =
            Manager::open_with(&root.join("data"), settings.clone(), link.clone(), timing())
                .unwrap();
        Self {
            root,
            settings,
            link,
            manage,
        }
    }

    fn switch(&self, json: &str) -> Result<(), String> {
        self.manage.apply_settings(Update::parse(json).unwrap())
    }

    fn switch_on(&self) -> Result<(), String> {
        self.switch(r#"{"collection":{"enabled":true}}"#)
    }

    fn collecting(&self) -> bool {
        self.settings.collection().enabled
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn calls(host: &FakeHost) -> Vec<(String, String)> {
    host.calls
        .lock()
        .unwrap()
        .iter()
        .map(|c| (c.method.clone(), c.path.clone()))
        .collect()
}

fn route(rest: &str) -> String {
    format!(
        "/construct/v1/subjects/{}/{rest}",
        toolkit_security::constants::DEFAULT_SUBJECT_ID
    )
}

fn reply(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn the_erase_command_calls_the_erasure_route_and_turns_all_collection_off() {
    let stand = Stand::new();
    let host = FakeHost::start();
    stand.link.set(host.connection());
    host.answer(200, SETTINGS_ON);
    host.answer(202, ERASING);
    assert!(stand.collecting());

    let answer = reply(&stand.manage.erase());
    assert_eq!(answer["erasure"], "started", "{answer}");
    assert_eq!(answer["collection_enabled"], false);
    assert!(!stand.collecting());
    assert_eq!(
        calls(&host),
        [
            ("GET".to_string(), route("settings")),
            ("POST".to_string(), route("erasure")),
        ]
    );
    let calls = host.calls.lock().unwrap();
    assert!(
        calls
            .iter()
            .all(|c| c.authorization == format!("Bearer {TOKEN}"))
    );
}

#[test]
fn a_switch_on_during_the_erasure_is_refused() {
    let stand = Stand::new();
    let host = FakeHost::start();
    stand.link.set(host.connection());
    set_collection(&stand.settings, r#"{"enabled":false}"#);
    host.answer(200, ERASING);

    let reason = stand.switch_on().unwrap_err();
    assert_eq!(reason, "collection stays off: an erasure is still running");
    assert!(!stand.collecting());
    assert_eq!(calls(&host), [("GET".to_string(), route("settings"))]);
}

#[test]
fn after_the_erasure_a_switch_on_turns_personalization_on_before_anything_is_collected() {
    let stand = Stand::new();
    let host = FakeHost::start();
    stand.link.set(host.connection());
    set_collection(&stand.settings, r#"{"enabled":false}"#);
    host.answer(200, SETTINGS_OFF);
    host.answer(200, SETTINGS_ON);
    // The answer to the second call is held back, so the state in between can be looked at.
    *host.delay.lock().unwrap() = Duration::from_millis(400);

    let stand = std::sync::Arc::new(stand);
    let switching = {
        let stand = std::sync::Arc::clone(&stand);
        thread::spawn(move || stand.switch_on())
    };
    wait_for("the call that turns personalization on", || {
        host.count() == 2
    });
    assert!(
        !stand.collecting(),
        "collection was on before the host had personalization on"
    );
    switching.join().unwrap().unwrap();
    assert!(stand.collecting());

    assert_eq!(
        calls(&host),
        [
            ("GET".to_string(), route("settings")),
            ("PUT".to_string(), route("settings")),
        ]
    );
    assert_eq!(
        host.calls.lock().unwrap()[1].body,
        json!({ "personalization_enabled": true })
    );
}

#[test]
fn a_switch_on_leaves_personalization_alone_when_it_is_on() {
    let stand = Stand::new();
    let host = FakeHost::start();
    stand.link.set(host.connection());
    set_collection(&stand.settings, r#"{"enabled":false}"#);
    host.answer(200, SETTINGS_ON);

    stand.switch_on().unwrap();
    assert!(stand.collecting());
    assert_eq!(calls(&host), [("GET".to_string(), route("settings"))]);
}

#[test]
fn an_unreachable_host_leaves_the_switch_off_with_the_reason() {
    let stand = Stand::new();
    set_collection(&stand.settings, r#"{"enabled":false}"#);

    // The host has not announced itself yet.
    let reason = stand.switch_on().unwrap_err();
    assert_eq!(
        reason,
        "collection stays off: the Construct host is not ready yet"
    );
    assert!(!stand.collecting());

    // The host is announced, but nothing listens on its port.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    stand.link.set(Connection {
        port,
        token: TOKEN.into(),
    });
    let reason = stand.switch_on().unwrap_err();
    assert_eq!(
        reason,
        "collection stays off: the Construct host cannot be reached"
    );
    assert!(!stand.collecting());

    // Other changes are not held up by the host.
    stand.switch(r#"{"collection":{"codex":false}}"#).unwrap();
    assert!(!stand.settings.collection().codex);
}

#[test]
fn a_host_that_refuses_the_switch_leaves_it_off() {
    let stand = Stand::new();
    let host = FakeHost::start();
    stand.link.set(host.connection());
    set_collection(&stand.settings, r#"{"enabled":false}"#);
    host.answer(200, SETTINGS_OFF);
    host.answer(503, "{}");

    let reason = stand.switch_on().unwrap_err();
    assert_eq!(
        reason,
        "collection stays off: the Construct host answered 503"
    );
    assert!(!stand.collecting());
}

#[test]
fn a_second_erase_request_while_one_runs_changes_nothing() {
    let stand = Stand::new();
    let host = FakeHost::start();
    stand.link.set(host.connection());
    host.answer(200, SETTINGS_ON);
    host.answer(202, ERASING);
    assert_eq!(reply(&stand.manage.erase())["erasure"], "started");
    assert!(!stand.collecting());

    // Collection is put back on behind the agent's back, so the second request has something to
    // change if it wrongly acts: it must leave collection as it finds it.
    set_collection(&stand.settings, r#"{"enabled":true}"#);
    host.answer(200, ERASING);
    let second = reply(&stand.manage.erase());
    assert_eq!(second["erasure"], "running", "{second}");
    assert_eq!(second["collection_enabled"], true);
    assert!(stand.collecting());
    let posts = calls(&host)
        .iter()
        .filter(|(method, _)| method == "POST")
        .count();
    assert_eq!(posts, 1, "the erasure was asked for twice");
    assert_eq!(host.count(), 3);
}

/// A prompt typed after the manager opened, as the readers store it.
fn store_live_prompt(stand: &Stand, text: &str) {
    let time = time::OffsetDateTime::now_utc() + time::Duration::seconds(5);
    let turn = Turn {
        tool: "claude-code".into(),
        session: "s1".into(),
        time: time
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap(),
        folder: String::new(),
        branch: String::new(),
        role: Role::User,
        author: Author::Unknown,
        text: text.into(),
        tools: Vec::new(),
    };
    let mut sink = FileSink::new(&stand.root.join("data"));
    sink.append(&Record::Turn(turn)).unwrap();
    sink.save_progress(&Progress::default()).unwrap();
}

#[test]
fn a_prompt_typed_before_the_erasure_and_not_sent_stays_unsent_after_a_restart() {
    let stand = Stand::new();
    let host = FakeHost::start();
    stand.link.set(host.connection());
    store_live_prompt(&stand, "an earlier prompt, never sent");
    host.answer(200, SETTINGS_ON);
    host.answer(202, ERASING);
    assert_eq!(reply(&stand.manage.erase())["erasure"], "started");

    // The agent restarts and the person has turned collection on again: the old prompt is
    // not queued, so the host hears nothing but the two calls of the erase.
    set_collection(&stand.settings, r#"{"enabled":true}"#);
    let restarted = Manager::open_with(
        &stand.root.join("data"),
        stand.settings.clone(),
        stand.link.clone(),
        timing(),
    )
    .unwrap();
    restarted.spawn();
    // The sender runs with a model and a host: a prompt that was queued would be sent within this
    // time, because the pauses of the test manager are tens of milliseconds.
    thread::sleep(Duration::from_secs(1));
    assert_eq!(host.count(), 2, "{:?}", calls(&host));
    assert_eq!(restarted.status()["refused"], 1);
}

#[test]
fn an_erase_that_cannot_reach_the_host_changes_nothing() {
    let stand = Stand::new();
    let answer = reply(&stand.manage.erase());
    assert!(answer["error"].is_string(), "{answer}");
    assert!(stand.collecting());
}

#[test]
fn an_erasure_the_host_refuses_leaves_collection_off_and_says_so() {
    let stand = Stand::new();
    let host = FakeHost::start();
    stand.link.set(host.connection());
    host.answer(200, SETTINGS_ON);
    host.answer(501, "{}");

    let answer = reply(&stand.manage.erase());
    assert_eq!(
        answer["error"],
        "collection is off, but the erasure did not start: the Construct host answered 501"
    );
    assert!(!stand.collecting());
}

/// The commands over the agent's socket, the way the app sends them.
struct Socket {
    path: PathBuf,
}

fn serve(stand: &Stand) -> Socket {
    let path = stand.root.join("run").join("agent.sock");
    let server = construct_agent::ipc::Server::bind(&path)
        .unwrap()
        .with_settings(stand.settings.clone())
        .with_manage(stand.manage.clone());
    thread::spawn(move || server.serve());
    Socket { path }
}

impl Socket {
    fn ask(&self, line: &str) -> Value {
        let text =
            construct_agent::ipc::request(&self.path, line, Duration::from_secs(20)).unwrap();
        reply(&text)
    }
}

#[test]
fn the_socket_serves_memory_erase_and_holds_the_switch_back_during_an_erasure() {
    let stand = Stand::new();
    let host = FakeHost::start();
    stand.link.set(host.connection());
    let socket = serve(&stand);

    host.answer(200, SETTINGS_ON);
    host.answer(202, ERASING);
    let erased = socket.ask("memory-erase");
    assert_eq!(erased["erasure"], "started", "{erased}");
    assert!(!stand.collecting());

    host.answer(200, ERASING);
    let refused = socket.ask(r#"settings-set {"collection":{"enabled":true}}"#);
    assert_eq!(
        refused["error"],
        "collection stays off: an erasure is still running"
    );
    assert!(!stand.collecting());

    host.answer(200, SETTINGS_OFF);
    host.answer(200, SETTINGS_ON);
    let on = socket.ask(r#"settings-set {"collection":{"enabled":true}}"#);
    assert_eq!(on["collection"]["enabled"], true, "{on}");
    assert!(stand.collecting());
}
