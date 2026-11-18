//! Prompts to the real Construct host, through the real agent binary: a fixture history and a live
//! prompt become facts in the profile (a fake model server answers the planner with a canned plan),
//! and a `kill -9` of the agent and of PostgreSQL loses nothing and repeats nothing. These tests
//! unpack and start PostgreSQL, so they are ignored by default: `cargo test -- --ignored`.

#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::io::Write as _;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use construct_agent::ipc::request;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

mod common;

use common::Agent;

const LANGUAGE_AND_REPOSITORY: &str =
    "I write Rust, and my code lives in the gears-rust repository";

const REMEMBER_CATS: &str = "remember that I love cats";

/// A request to remember something else; the fake model answers it as it answers `REMEMBER_CATS`.
const REMEMBER_CATS_AGAIN: &str = "remember that I love cats, again";

/// A model server that answers the planner: a prompt that names a repository gets two facts, a
/// request to remember something gets a note, any other prompt gets "nothing changed". It counts
/// the conversations the planner started.
struct FakeModel {
    url: String,
    conversations: Arc<AtomicUsize>,
    /// How long each answer is held back.
    delay: Arc<std::sync::Mutex<Duration>>,
}

impl FakeModel {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let conversations = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&conversations);
        let delay = Arc::new(std::sync::Mutex::new(Duration::ZERO));
        let held = Arc::clone(&delay);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let counter = Arc::clone(&counter);
                let held = Arc::clone(&held);
                thread::spawn(move || {
                    let (body, _) = common::read_request(&mut stream);
                    // The chat asks for a stream: it gets a short text, and is not counted.
                    if body.contains(r#""stream":true"#) {
                        common::head(&mut stream);
                        common::raw_chunk(
                            &mut stream,
                            &json!({ "choices": [{ "delta": { "content": "Noted." } }] }),
                        );
                        common::raw_chunk(
                            &mut stream,
                            &json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }),
                        );
                        common::done(&mut stream);
                        return;
                    }
                    let answered_tools = body.contains(r#""role":"tool""#);
                    if !answered_tools {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                    thread::sleep(*held.lock().unwrap());
                    let message = if body.contains(REMEMBER_CATS) && !answered_tools {
                        json!({
                            "role": "assistant",
                            "content": null,
                            "tool_calls": [call("call_1", "add", &json!({
                                "property": "remembered_notes",
                                "value": "The person loves cats",
                                "confidence": 0.9,
                            }))],
                        })
                    } else if body.contains("gears-rust") && !answered_tools {
                        json!({
                            "role": "assistant",
                            "content": null,
                            "tool_calls": [
                                call("call_1", "add", &json!({
                                    "property": "programming_languages",
                                    "value": { "name": "Rust" },
                                    "confidence": 0.9,
                                })),
                                call("call_2", "add", &json!({
                                    "property": "repositories",
                                    "value": "gears-rust",
                                    "confidence": 0.9,
                                })),
                            ],
                        })
                    } else {
                        json!({ "role": "assistant", "content": "Nothing changed." })
                    };
                    let reason = if message["content"].is_null() {
                        "tool_calls"
                    } else {
                        "stop"
                    };
                    let answer = json!({
                        "id": "chatcmpl-test",
                        "object": "chat.completion",
                        "choices": [{ "index": 0, "message": message, "finish_reason": reason }],
                        "usage": { "prompt_tokens": 100, "completion_tokens": 10, "total_tokens": 110 },
                    })
                    .to_string();
                    common::status(&mut stream, 200, &answer);
                });
            }
        });
        Self {
            url,
            conversations,
            delay,
        }
    }

    fn conversations(&self) -> usize {
        self.conversations.load(Ordering::SeqCst)
    }
}

fn call(id: &str, name: &str, arguments: &Value) -> Value {
    json!({
        "id": id,
        "type": "function",
        "function": { "name": name, "arguments": arguments.to_string() },
    })
}

/// A scratch user: a home with the tools' logs, and the agent's data, config and runtime directories.
struct Stand {
    root: PathBuf,
}

impl Stand {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("cb-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("run")).unwrap();
        std::fs::create_dir_all(root.join("home")).unwrap();
        std::fs::set_permissions(
            root.join("run"),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        Self { root }
    }

    fn agent(&self) -> Agent {
        Agent(
            Command::new(env!("CARGO_BIN_EXE_construct-agent"))
                .arg("run")
                .env("HOME", self.root.join("home"))
                .env("XDG_DATA_HOME", self.root.join("data"))
                .env("XDG_CONFIG_HOME", self.root.join("config"))
                .env("XDG_RUNTIME_DIR", self.root.join("run"))
                .env_remove("CLAUDE_CONFIG_DIR")
                .env_remove("CODEX_HOME")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }

    fn socket(&self) -> PathBuf {
        self.root.join("run/construct/agent.sock")
    }

    /// One command to the agent; the reply is JSON.
    fn ask(&self, line: &str) -> Value {
        let reply = request(&self.socket(), line, Duration::from_secs(60)).unwrap();
        serde_json::from_str(&reply).unwrap()
    }

    fn status(&self) -> Value {
        self.ask("status")
    }

    fn wait(&self, what: &str, mut done: impl FnMut(&Value) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(300);
        while Instant::now() < deadline {
            if let Ok(reply) = request(&self.socket(), "status", Duration::from_secs(5))
                && let Ok(status) = serde_json::from_str::<Value>(&reply)
                && done(&status)
            {
                return;
            }
            thread::sleep(Duration::from_millis(200));
        }
        panic!("timed out waiting for {what}: {}", self.status());
    }

    fn wait_ready(&self) {
        self.wait("construct", |s| s["construct"]["state"] == "ready");
    }

    fn set_model(&self, url: &str) {
        self.wait("the socket", |_| true);
        let update = json!({ "model": { "base_url": url, "model": "m", "api_key": "k" } });
        let reply = self.ask(&format!("settings-set {update}"));
        assert!(reply.get("error").is_none(), "{reply}");
    }

    fn connection(&self) -> (u16, String) {
        let file = self.root.join("run/construct/connection.json");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        (
            u16::try_from(v["port"].as_u64().unwrap()).unwrap(),
            v["token"].as_str().unwrap().to_string(),
        )
    }

    /// The profile of the static identity, as the app reads it.
    fn profile(&self) -> Value {
        let (port, token) = self.connection();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .proxy(None)
            .http_status_as_error(false)
            .build()
            .into();
        let subject = toolkit_security::constants::DEFAULT_SUBJECT_ID;
        let mut response = agent
            .get(&format!(
                "http://127.0.0.1:{port}/construct/v1/subjects/{subject}/profile"
            ))
            .header("Authorization", format!("Bearer {token}"))
            .call()
            .unwrap();
        assert_eq!(response.status().as_u16(), 200);
        serde_json::from_str(&response.body_mut().read_to_string().unwrap()).unwrap()
    }

    /// One call to a route of the person's own data, as the app makes it: the answer's status and its
    /// JSON body (`Null` when there is none).
    fn own(&self, method: &str, rest: &str, body: Option<&Value>) -> (u16, Value) {
        let (port, token) = self.connection();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .proxy(None)
            .http_status_as_error(false)
            .build()
            .into();
        let subject = toolkit_security::constants::DEFAULT_SUBJECT_ID;
        let url = format!("http://127.0.0.1:{port}/construct/v1/subjects/{subject}/{rest}");
        let auth = format!("Bearer {token}");
        let mut response = match (method, body) {
            ("GET", _) => agent.get(&url).header("Authorization", auth).call(),
            ("DELETE", _) => agent.delete(&url).header("Authorization", auth).call(),
            ("POST", _) => agent.post(&url).header("Authorization", auth).send_empty(),
            ("PUT", Some(body)) => agent
                .put(&url)
                .header("Authorization", auth)
                .header("Content-Type", "application/json")
                .send(body.to_string()),
            _ => panic!("no such call: {method}"),
        }
        .unwrap();
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string().unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    /// The facts the person sees in the list, as `(fact_id, property)`.
    fn listed(&self) -> Vec<(String, String)> {
        let (status, page) = self.own("GET", "facts", None);
        assert_eq!(status, 200, "{page}");
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                (
                    f["fact_id"].as_str().unwrap().to_string(),
                    f["property"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    /// Whether the reader of the Claude Code log has saved a position at the end of it.
    fn log_is_read_to_the_end(&self) -> bool {
        let sink = construct_agent::records::FileSink::new(&self.root.join("data/construct"));
        let length = std::fs::metadata(self.claude_log()).unwrap().len();
        construct_agent::records::Sink::load_progress(&sink)
            .unwrap()
            .files
            .iter()
            .any(|(path, progress)| path.ends_with("cc1.jsonl") && progress.offset == length)
    }

    fn postmaster_pid(&self) -> Option<u32> {
        let file = self.root.join("data/construct/postgres/postmaster.pid");
        std::fs::read_to_string(file)
            .ok()?
            .lines()
            .next()?
            .parse()
            .ok()
    }

    fn claude_log(&self) -> PathBuf {
        self.root.join("home/.claude/projects/p/cc1.jsonl")
    }

    /// 15 prompts of Claude Code and 10 of Codex from the last days, each answered.
    fn write_history(&self) {
        let now = OffsetDateTime::now_utc();
        let at = |minutes: i64| {
            (now - time::Duration::days(2) + time::Duration::minutes(minutes))
                .format(&Rfc3339)
                .unwrap()
        };
        let mut claude = String::new();
        for i in 0..15 {
            claude.push_str(&claude_line(
                "user",
                &at(i * 2),
                &format!("claude prompt {i:02}"),
            ));
            claude.push_str(&claude_line("assistant", &at(i * 2 + 1), "done"));
        }
        std::fs::create_dir_all(self.claude_log().parent().unwrap()).unwrap();
        std::fs::write(self.claude_log(), claude).unwrap();

        let id = "0190f7a2-0000-7000-8000-000000000001";
        let codex_file = self.root.join(format!(
            "home/.codex/sessions/2026/11/15/rollout-2026-11-15T10-00-00-{id}.jsonl"
        ));
        std::fs::create_dir_all(codex_file.parent().unwrap()).unwrap();
        let mut codex = String::new();
        for i in 0..10 {
            codex.push_str(&codex_line(
                "user",
                &at(100 + i * 2),
                &format!("codex prompt {i:02}"),
            ));
            codex.push_str(&codex_line("assistant", &at(101 + i * 2), "done"));
        }
        std::fs::write(codex_file, codex).unwrap();
    }

    /// A prompt typed now, after the agent started.
    fn type_live_prompt(&self, text: &str) {
        let now = OffsetDateTime::now_utc().format(&Rfc3339).unwrap();
        let mut log = std::fs::OpenOptions::new()
            .append(true)
            .open(self.claude_log())
            .unwrap();
        log.write_all(claude_line("user", &now, text).as_bytes())
            .unwrap();
    }
}

fn claude_line(role: &str, time: &str, text: &str) -> String {
    let content = if role == "user" {
        json!(text)
    } else {
        json!([{ "type": "text", "text": text }])
    };
    format!(
        "{}\n",
        json!({ "type": role, "timestamp": time, "sessionId": "cc1", "cwd": "/work",
                "message": { "role": role, "content": content } })
    )
}

fn codex_line(role: &str, time: &str, text: &str) -> String {
    let kind = if role == "user" {
        "input_text"
    } else {
        "output_text"
    };
    format!(
        "{}\n",
        json!({ "timestamp": time, "type": "response_item",
                "payload": { "type": "message", "role": role, "content": [{ "type": kind, "text": text }] } })
    )
}

impl Drop for Stand {
    fn drop(&mut self) {
        if let Some(pid) = self.postmaster_pid() {
            let _ = Command::new("kill").arg(pid.to_string()).status();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// The values of a property in the profile, whatever the category.
fn values_of(profile: &Value, property: &str) -> Vec<Value> {
    profile["categories"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|c| c["facts"].as_array().unwrap().iter())
        .filter(|f| f["property"] == property)
        .map(|f| f["value"].clone())
        .collect()
}

fn scanned(status: &Value) -> bool {
    ["claude-code", "codex"]
        .iter()
        .all(|tool| status["connectors"][tool]["scanning"] == false)
}

#[test]
#[ignore = "starts PostgreSQL and the Construct host; run with --ignored"]
fn prompts_become_facts_and_a_kill_9_loses_and_repeats_nothing() {
    let stand = Stand::new("mf");
    let model = FakeModel::start();
    stand.write_history();
    let mut agent = stand.agent();
    stand.set_model(&model.url);
    stand.wait_ready();
    stand.wait("the first scan", scanned);

    // The history is counted per tool and nothing is sent by itself.
    let dry = stand.ask(r#"backfill {"dry_run":true}"#);
    assert_eq!(
        dry["prompts"],
        json!({ "chat": 0, "claude-code": 15, "codex": 10 }),
        "{dry}"
    );
    assert_eq!(dry["total"], 25);
    assert_eq!(model.conversations(), 0, "history went out by itself");
    assert!(values_of(&stand.profile(), "repositories").is_empty());

    // A prompt typed after the first start goes out and the profile can be read back.
    stand.type_live_prompt(LANGUAGE_AND_REPOSITORY);
    stand.wait("the live prompt", |s| s["facts"]["live"]["stored"] == 1);
    let profile = stand.profile();
    assert_eq!(values_of(&profile, "repositories"), [json!("gears-rust")]);
    assert_eq!(
        values_of(&profile, "programming_languages")[0]["name"],
        "Rust"
    );
    assert_eq!(model.conversations(), 1);

    // A small first run, then the rest after a crash.
    let started = stand.ask(r#"backfill {"limit":20}"#);
    assert_eq!(started["queued"], 20, "{started}");
    stand.wait("the first backfill", |s| {
        s["facts"]["backfill"]["state"] == "done"
    });
    assert_eq!(stand.status()["facts"]["backfill"]["sent"], 20);
    assert_eq!(model.conversations(), 21);

    let postmaster = stand.postmaster_pid().unwrap();
    kill_9(&mut agent);
    Command::new("kill")
        .args(["-9", &postmaster.to_string()])
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while alive(postmaster) {
        assert!(Instant::now() < deadline, "PostgreSQL did not die");
        thread::sleep(Duration::from_millis(100));
    }

    // Both come back, the data is there, and the next run sends only what is left.
    let _agent = stand.agent();
    stand.wait_ready();
    stand.wait("the first scan again", scanned);
    assert_eq!(
        values_of(&stand.profile(), "repositories"),
        [json!("gears-rust")],
        "the profile lost its facts"
    );
    let dry = stand.ask(r#"backfill {"dry_run":true}"#);
    assert_eq!(dry["total"], 5, "{dry}");
    assert_eq!(stand.status()["facts"]["sent"], 21);
    let started = stand.ask("backfill {}");
    assert_eq!(started["queued"], 5, "{started}");
    stand.wait("the second backfill", |s| {
        s["facts"]["backfill"]["state"] == "done"
    });
    assert_eq!(model.conversations(), 26, "a prompt was sent twice");
    assert_eq!(stand.status()["facts"]["sent"], 26);
    // Nothing is left, and a third run has nothing to do.
    assert_eq!(stand.ask(r#"backfill {"dry_run":true}"#)["total"], 0);
}

#[test]
#[ignore = "starts PostgreSQL and the Construct host; run with --ignored"]
fn a_kill_9_during_a_call_does_not_send_that_prompt_again() {
    let stand = Stand::new("mk");
    let model = FakeModel::start();
    stand.write_history();
    let mut agent = stand.agent();
    stand.set_model(&model.url);
    stand.wait_ready();
    stand.wait("the first scan", scanned);

    // Each prompt takes three seconds in the model, so the kill lands between the journal mark and
    // the answer of one.
    *model.delay.lock().unwrap() = Duration::from_secs(3);
    let started = stand.ask(r#"backfill {"limit":10}"#);
    assert_eq!(started["queued"], 10, "{started}");
    let deadline = Instant::now() + Duration::from_secs(60);
    while model.conversations() < 2 {
        assert!(Instant::now() < deadline, "the model got no second prompt");
        thread::sleep(Duration::from_millis(50));
    }
    let postmaster = stand.postmaster_pid().unwrap();
    kill_9(&mut agent);
    Command::new("kill")
        .args(["-9", &postmaster.to_string()])
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while alive(postmaster) {
        assert!(Instant::now() < deadline, "PostgreSQL did not die");
        thread::sleep(Duration::from_millis(100));
    }
    let seen_by_the_model = model.conversations();

    *model.delay.lock().unwrap() = Duration::ZERO;
    let _agent = stand.agent();
    stand.wait_ready();
    stand.wait("the first scan again", scanned);
    // The prompt that was in the call is marked as sent, and so is every one before it.
    let left = stand.ask(r#"backfill {"dry_run":true}"#)["total"]
        .as_u64()
        .unwrap();
    assert_eq!(usize::try_from(left).unwrap(), 25 - seen_by_the_model);
    let _ = stand.ask("backfill {}");
    stand.wait("the rest", |s| s["facts"]["backfill"]["state"] == "done");
    assert_eq!(
        model.conversations(),
        25,
        "a prompt reached the model twice"
    );
}

/// A chat message through the real agent: the planner is asked once and one message is stored.
fn send_chat_message(stand: &Stand, model: &FakeModel, message: &str) {
    let line = format!(
        "chat-send {}",
        json!({ "conversation": "", "message": message })
    );
    let reply = request(&stand.socket(), &line, Duration::from_secs(60)).unwrap();
    // The first line of the answer; the chat goes on by itself.
    assert!(reply.contains(r#""event":"start""#), "{reply}");
    stand.wait("the chat message", |s| s["facts"]["live"]["stored"] == 1);
    assert_eq!(model.conversations(), 1, "one conversation of the planner");
}

/// SIGKILL, then wait for the exit.
fn kill_9(agent: &mut Agent) {
    agent.0.kill().unwrap();
    agent.0.wait().unwrap();
}

#[test]
#[ignore = "starts PostgreSQL and the Construct host; run with --ignored"]
fn a_chat_message_in_the_real_agent_becomes_a_prompt_for_the_facts_route() {
    let stand = Stand::new("mf-chat");
    let model = FakeModel::start();
    let _agent = stand.agent();
    stand.set_model(&model.url);
    stand.wait_ready();

    send_chat_message(&stand, &model, LANGUAGE_AND_REPOSITORY);
    assert_eq!(
        values_of(&stand.profile(), "repositories"),
        [json!("gears-rust")]
    );
}

#[test]
#[ignore = "starts PostgreSQL and the Construct host; run with --ignored"]
fn a_request_to_remember_lands_in_the_remembered_notes_property() {
    let stand = Stand::new("mf-notes");
    let model = FakeModel::start();
    let _agent = stand.agent();
    stand.set_model(&model.url);
    stand.wait_ready();

    send_chat_message(&stand, &model, REMEMBER_CATS);
    let profile = stand.profile();
    assert_eq!(
        values_of(&profile, "remembered_notes"),
        [json!("The person loves cats")]
    );
    // The note is the only fact the message produced.
    let facts: usize = profile["categories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["facts"].as_array().unwrap().len())
        .sum();
    assert_eq!(facts, 1, "{profile}");
}

/// Stop the agent and PostgreSQL the hard way and wait until both are gone.
fn kill_agent_and_database(stand: &Stand, agent: &mut Agent) {
    let postmaster = stand.postmaster_pid().unwrap();
    kill_9(agent);
    Command::new("kill")
        .args(["-9", &postmaster.to_string()])
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while alive(postmaster) {
        assert!(Instant::now() < deadline, "PostgreSQL did not die");
        thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "starts PostgreSQL and the Construct host; run with --ignored"]
fn the_persons_own_routes_answer_to_the_token_and_the_erasure_empties_the_list() {
    // The erasure cannot be undone: it runs here only against the host of this test, which lives
    // in a scratch directory of its own.
    let stand = Stand::new("mf-own");
    let model = FakeModel::start();
    stand.write_history();
    let mut agent = stand.agent();
    stand.set_model(&model.url);
    stand.wait_ready();
    stand.wait("the first scan", scanned);

    // The settings route answers, and personalization is on.
    let (status, settings) = stand.own("GET", "settings", None);
    assert_eq!(status, 200, "{settings}");
    assert_eq!(settings["personalization_enabled"], true);
    assert_eq!(settings["erasure_in_progress"], false);

    // Two facts from one prompt: the list shows them, and the delete route removes one.
    stand.type_live_prompt(LANGUAGE_AND_REPOSITORY);
    stand.wait("the live prompt", |s| s["facts"]["live"]["stored"] == 1);
    let facts = stand.listed();
    assert_eq!(facts.len(), 2, "{facts:?}");
    let (repository_id, _) = facts.iter().find(|(_, p)| p == "repositories").unwrap();
    let (status, _) = stand.own("DELETE", &format!("facts/{repository_id}"), None);
    assert_eq!(status, 204);
    assert!(stand.listed().iter().all(|(_, p)| p != "repositories"));

    // The same prompt is read again after a restart: the journal of what was sent keeps the
    // deleted fact from being made again.
    kill_agent_and_database(&stand, &mut agent);
    let _agent = stand.agent();
    stand.wait_ready();
    stand.wait("the first scan again", scanned);

    // A second prompt, then the erasure: collection goes off and the list ends up empty. Prompts
    // go out oldest first, so once the second one has been sent, a resend of the first would
    // already have been counted: the model has seen exactly two.
    stand.type_live_prompt(REMEMBER_CATS);
    wait_until("the second prompt", || model.conversations() >= 2);
    wait_until("its fact", || stand.listed().len() == 2);
    assert_eq!(model.conversations(), 2, "the first prompt was sent again");
    assert!(stand.listed().iter().all(|(_, p)| p != "repositories"));
    let erased = stand.ask("memory-erase");
    assert_eq!(erased["erasure"], "started", "{erased}");
    assert_eq!(erased["collection_enabled"], false);
    assert_eq!(stand.ask("settings-get")["collection"]["enabled"], false);
    wait_until("the erasure to end", || {
        let (status, settings) = stand.own("GET", "settings", None);
        status == 200 && settings["erasure_in_progress"] == false
    });
    assert!(stand.listed().is_empty());
    // Personalization stays off in the host until the person turns collection on again.
    assert_eq!(
        stand.own("GET", "settings", None).1["personalization_enabled"],
        false
    );

    // A prompt typed while collection is off is read and passed over: the reader's saved position
    // reaches the end of the log, and nothing is stored or sent.
    stand.type_live_prompt("I started a repository called gears-rust-two");
    wait_until("the reader to pass the prompt", || {
        stand.log_is_read_to_the_end()
    });
    assert_eq!(model.conversations(), 2);

    // The switch-on turns personalization on in the host first; a new prompt makes a fact again.
    let on = stand.ask(r#"settings-set {"collection":{"enabled":true}}"#);
    assert!(on.get("error").is_none(), "{on}");
    assert_eq!(on["collection"]["enabled"], true);
    assert_eq!(
        stand.own("GET", "settings", None).1["personalization_enabled"],
        true
    );
    // A reader decides at the start of a pass whether it stores: it has to have seen the switch.
    stand.wait("the readers to see the switch", |s| {
        s["connectors"]["claude-code"].get("off").is_none()
    });
    stand.type_live_prompt(REMEMBER_CATS_AGAIN);
    wait_until("the new prompt", || model.conversations() == 3);
    wait_until("a fact again", || stand.listed().len() == 1);
    // The refused prompt names a repository: if it had been sent, the model would have made one.
    assert_eq!(model.conversations(), 3);
    assert!(stand.listed().iter().all(|(_, p)| p != "repositories"));
}

/// Waits until `done`, five minutes at most.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(300);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(200));
    }
}
