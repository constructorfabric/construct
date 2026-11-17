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

/// A model server that answers the planner: a prompt that names a repository gets two facts, any
/// other prompt gets "nothing changed". It counts the conversations the planner started.
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
                    let message = if body.contains("gears-rust") && !answered_tools {
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
        self.wait("construct", |s| s["construct"] == "ready");
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

    let line = format!(
        "chat-send {}",
        json!({ "conversation": "", "message": LANGUAGE_AND_REPOSITORY })
    );
    let reply = request(&stand.socket(), &line, Duration::from_secs(60)).unwrap();
    // The first line of the answer; the chat goes on by itself.
    assert!(reply.contains(r#""event":"start""#), "{reply}");
    stand.wait("the chat message", |s| s["facts"]["live"]["stored"] == 1);
    assert_eq!(model.conversations(), 1, "one conversation of the planner");
    assert_eq!(
        values_of(&stand.profile(), "repositories"),
        [json!("gears-rust")]
    );
}
