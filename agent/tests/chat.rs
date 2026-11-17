//! The chat commands of the socket against a fake provider on a local port: the provider
//! sends `data:` lines with pauses and ends with `data: [DONE]`.
#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use connectors::{Record, ToolCall, ToolResult, compare_times};
use construct_agent::chat::tool::{ChatTool, ChatTools};
use construct_agent::chat::{Chat, Deadlines};
use construct_agent::ipc::Server;
use construct_agent::records::{FileSink, Sink};
use construct_agent::settings::{Settings, Update};
use serde_json::{Value, json};

mod common;

use common::stand::deadlines as short;
use common::{chunk, done, head, hold, raw_chunk, read_request, status};

const KEY: &str = "sk-test-key-4f9a";
const MESSAGE: &str = "my secret plan is the blue door";

/// What a fake tool does with its name and the arguments text.
type Runner = Arc<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>;

/// A tool that hands its call to a `Runner`.
struct Fakeware {
    name: &'static str,
    runner: Runner,
}

impl ChatTool for Fakeware {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &'static str {
        "A tool for the tests."
    }

    fn schema(&self) -> &'static str {
        r#"{"type":"object","properties":{"q":{"type":"string"}}}"#
    }

    /// `Read <q>` for arguments that have a `q`, the name for any other.
    fn line(&self, arguments: &str) -> String {
        serde_json::from_str::<Value>(arguments)
            .ok()
            .and_then(|v| v["q"].as_str().map(|q| format!("Read {q}")))
            .unwrap_or_else(|| self.name.to_string())
    }

    fn run(&self, arguments: &str) -> Result<String, String> {
        (self.runner)(self.name, arguments)
    }
}

/// The names the tests call; each is a tool that goes to the same runner.
const NAMES: [&str; 7] = ["fail", "first", "lookup", "now", "other", "quick", "slow"];

// The callers hand over their runner.
#[allow(clippy::needless_pass_by_value)]
fn registry(runner: Runner) -> ChatTools {
    ChatTools::new(
        NAMES
            .iter()
            .map(|&name| {
                Arc::new(Fakeware {
                    name,
                    runner: Arc::clone(&runner),
                }) as Arc<dyn ChatTool>
            })
            .collect(),
    )
    .unwrap()
}

/// A runner whose calls to `only` (every call when `None`) block until the test lets them go, or
/// for five seconds at most. The receiver gets a note when a call starts, the sender releases.
fn blocking(only: Option<&'static str>) -> (Runner, mpsc::Receiver<()>, mpsc::Sender<()>) {
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let started_tx = Mutex::new(started_tx);
    let release_rx = Mutex::new(release_rx);
    let runner: Runner = Arc::new(move |name, _| {
        if only.is_none_or(|only| only == name) {
            let _ = started_tx.lock().unwrap().send(());
            let _ = release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5));
        }
        Ok(format!("{name} done"))
    });
    (runner, started_rx, release_tx)
}

/// What the fake provider does with one request: write to the connection.
type Script = Arc<dyn Fn(usize, &mut TcpStream) + Send + Sync>;

struct Fake {
    addr: SocketAddr,
    bodies: Arc<Mutex<Vec<String>>>,
    auth: Arc<Mutex<Vec<String>>>,
}

impl Fake {
    fn start(script: Script) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let auth = Arc::new(Mutex::new(Vec::new()));
        let (b, a) = (Arc::clone(&bodies), Arc::clone(&auth));
        thread::spawn(move || {
            let count = Arc::new(AtomicU32::new(0));
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let (body, authorization) = read_request(&mut stream);
                b.lock().unwrap().push(body);
                a.lock().unwrap().push(authorization);
                let n = count.fetch_add(1, Ordering::SeqCst) as usize;
                let script = Arc::clone(&script);
                thread::spawn(move || script(n, &mut stream));
            }
        });
        Self { addr, bodies, auth }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.addr.port())
    }

    fn bodies(&self) -> Vec<String> {
        self.bodies.lock().unwrap().clone()
    }
}

struct Env {
    chat: Chat,
    socket: PathBuf,
    settings: Settings,
    data: PathBuf,
    rt: tokio::runtime::Runtime,
}

impl Env {
    fn new(name: &str, base_url: &str, deadlines: Deadlines) -> Self {
        Self::build(name, base_url, deadlines, None)
    }

    fn with_tools(name: &str, base_url: &str, runner: Runner) -> Self {
        Self::assemble(name, base_url, short(), None, Some(registry(runner)))
    }

    fn build(name: &str, base_url: &str, deadlines: Deadlines, wire_limit: Option<usize>) -> Self {
        Self::assemble(name, base_url, deadlines, wire_limit, None)
    }

    fn assemble(
        name: &str,
        base_url: &str,
        deadlines: Deadlines,
        wire_limit: Option<usize>,
        tools: Option<ChatTools>,
    ) -> Self {
        let base =
            std::env::temp_dir().join(format!("construct-chat-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let socket = base.join("run").join("construct").join("agent.sock");
        let data = base.join("data");
        let settings = Settings::open(&base.join("config")).unwrap();
        let update = json!({ "model": { "base_url": base_url, "model": "m", "api_key": KEY } });
        settings
            .apply(Update::parse(&update.to_string()).unwrap())
            .unwrap();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut chat = Chat::new(settings.clone(), &data, rt.handle().clone(), deadlines).unwrap();
        if let Some(limit) = wire_limit {
            chat = chat.with_wire_limit(limit);
        }
        if let Some(tools) = tools {
            chat = chat.with_tools(tools);
        }
        let server = Server::bind(&socket)
            .unwrap()
            .with_settings(settings.clone())
            .with_chat(chat.clone());
        thread::spawn(move || server.serve());
        Self {
            chat,
            socket,
            settings,
            data,
            rt,
        }
    }

    fn runtime_handle(&self) -> tokio::runtime::Handle {
        self.rt.handle().clone()
    }

    fn connect(&self, request: &str) -> Lines {
        let mut s = UnixStream::connect(&self.socket).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        s.write_all(request.as_bytes()).unwrap();
        s.write_all(b"\n").unwrap();
        Lines(BufReader::new(s))
    }

    fn send(&self, conversation: &str, message: &str) -> Lines {
        self.connect(&format!(
            "chat-send {}",
            json!({ "conversation": conversation, "message": message })
        ))
    }

    /// What `chat-get` sends, the entries of role `tool` left out.
    fn get(&self, conversation: &str) -> Vec<Value> {
        let mut entries = self.get_all(conversation);
        entries.retain(|e| e["role"] != "tool");
        entries
    }

    /// What `chat-get` sends.
    fn get_all(&self, conversation: &str) -> Vec<Value> {
        self.connect(&format!("chat-get {conversation}")).rest()
    }
}

struct Lines(BufReader<UnixStream>);

impl Lines {
    fn next(&mut self) -> Option<Value> {
        let mut line = String::new();
        if self.0.read_line(&mut line).unwrap() == 0 {
            return None;
        }
        Some(serde_json::from_str(&line).unwrap())
    }

    fn rest(mut self) -> Vec<Value> {
        std::iter::from_fn(|| self.next()).collect()
    }
}

/// The conversation id, the text of the deltas, and the last event of an answer.
fn summary(events: &[Value]) -> (String, String, Value) {
    let conversation = events
        .iter()
        .find(|e| e["event"] == "start")
        .map_or(String::new(), |e| {
            e["conversation"].as_str().unwrap().to_string()
        });
    let text = events
        .iter()
        .filter(|e| e["event"] == "delta")
        .map(|e| e["text"].as_str().unwrap())
        .collect();
    (conversation, text, events.last().unwrap().clone())
}

fn reason(events: &[Value]) -> String {
    let last = events.last().unwrap();
    assert_eq!(last["event"], "error", "{events:?}");
    last["reason"].as_str().unwrap().to_string()
}

fn assistant_text(records: &[Value]) -> Option<String> {
    records
        .iter()
        .rfind(|r| r["role"] == "assistant")
        .map(|r| r["text"].as_str().unwrap().to_string())
}

fn replying(words: &'static [&'static str]) -> Script {
    Arc::new(move |_, s| {
        head(s);
        for w in words {
            chunk(s, w);
        }
        done(s);
    })
}

#[test]
fn the_first_delta_arrives_before_the_last_chunk_is_sent() {
    let (tx, rx): (Sender<()>, Receiver<()>) = mpsc::channel();
    let rx = Arc::new(Mutex::new(rx));
    let released = Arc::new(Mutex::new(false));
    let flag = Arc::clone(&released);
    let fake = Fake::start(Arc::new(move |_, s| {
        head(s);
        chunk(s, "Hel");
        // The last chunk waits until the test client has seen the first delta.
        let got = rx
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(10))
            .is_ok();
        *flag.lock().unwrap() = got;
        chunk(s, "lo");
        done(s);
    }));
    let env = Env::new("stream", &fake.url(), short());
    let mut lines = env.send("", MESSAGE);
    assert_eq!(lines.next().unwrap()["event"], "start");
    let first = lines.next().unwrap();
    assert_eq!(first, json!({ "event": "delta", "text": "Hel" }));
    tx.send(()).unwrap();
    let mut events = vec![first];
    events.extend(lines.rest());
    assert!(*released.lock().unwrap());
    assert_eq!(events.last().unwrap(), &json!({ "event": "done" }));
    let text: String = events.iter().filter_map(|e| e["text"].as_str()).collect();
    assert_eq!(text, "Hello");

    // The answer is stored as the joined text, and the key went only in the header.
    let id = {
        let list = env.connect("chat-list").rest();
        list[0]["conversations"][0]["conversation"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let records = env.get(&id);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["role"], "user");
    assert_eq!(records[0]["text"], MESSAGE);
    assert_eq!(assistant_text(&records).as_deref(), Some("Hello"));
    let body: Value = serde_json::from_str(&fake.bodies()[0]).unwrap();
    assert_eq!(body["stream"], true);
    assert_eq!(body["model"], "m");
    assert!(body.get("max_tokens").is_none() && body.get("stream_options").is_none());
    assert_eq!(fake.auth.lock().unwrap()[0], format!("Bearer {KEY}"));
    assert!(!fake.bodies()[0].contains(KEY));
}

#[test]
fn refusals_have_their_reasons_and_leak_nothing() {
    let cases: Vec<(u16, &str, &str)> = vec![
        (401, "{}", "key refused"),
        (403, "{}", "key refused"),
        (404, "{}", "model not found"),
        (500, "{}", "error 500"),
        (
            400,
            r#"{"error":{"code":"context_length_exceeded","message":"too much"}}"#,
            "too long",
        ),
        (400, r#"{"error":{"message":"bad"}}"#, "error 400"),
    ];
    // The body of every refusal quotes the key and the message, as a provider may.
    let echo = format!("{KEY} {MESSAGE}");
    let table = Arc::new(cases.clone());
    let fake = Fake::start({
        let table = Arc::clone(&table);
        let echo = echo.clone();
        Arc::new(move |n, s| {
            let (code, body, _) = table[n];
            status(s, code, &format!("{{\"echo\":\"{echo}\",\"x\":{body}}}"));
        })
    });
    let env = Env::new("refusals", &fake.url(), short());
    for (_, _, expected) in cases {
        let events = env.send("", MESSAGE).rest();
        assert_eq!(reason(&events), expected);
        let all = serde_json::to_string(&events).unwrap();
        assert!(!all.contains(KEY) && !all.contains(MESSAGE), "{all}");
    }
}

#[test]
fn a_port_with_nothing_behind_it_is_unreachable() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let env = Env::new("closed", &format!("http://127.0.0.1:{port}/v1"), short());
    let events = env.send("", MESSAGE).rest();
    assert_eq!(reason(&events), "unreachable");
}

#[test]
fn a_provider_that_sends_nothing_hits_the_first_word_deadline() {
    let fake = Fake::start(Arc::new(|_, s| hold(s)));
    let deadlines = Deadlines {
        first_word: Duration::from_millis(500),
        ..short()
    };
    let env = Env::new("silent", &fake.url(), deadlines);
    let started = Instant::now();
    let events = env.send("", MESSAGE).rest();
    assert_eq!(reason(&events), "timeout");
    assert!(started.elapsed() < Duration::from_secs(5));
    // Headers without a first chunk are no better.
    let fake = Fake::start(Arc::new(|_, s| {
        head(s);
        hold(s);
    }));
    let env = Env::new("silent-head", &fake.url(), deadlines);
    assert_eq!(reason(&env.send("", MESSAGE).rest()), "timeout");
}

#[test]
fn a_dropped_connection_keeps_the_part_and_says_cut() {
    let fake = Fake::start(Arc::new(|_, s| {
        head(s);
        chunk(s, "one two");
        chunk(s, " three");
        // The connection closes here, without `[DONE]`.
    }));
    let env = Env::new("cut", &fake.url(), short());
    let events = env.send("", MESSAGE).rest();
    let (conversation, text, last) = summary(&events);
    assert_eq!(text, "one two three");
    assert_eq!(last, json!({ "event": "error", "reason": "cut" }));
    assert_eq!(
        assistant_text(&env.get(&conversation)).as_deref(),
        Some("one two three")
    );
}

#[test]
fn chat_stop_ends_a_never_ending_answer_and_keeps_the_part() {
    let fake = Fake::start(Arc::new(|_, s| {
        head(s);
        chunk(s, "part");
        hold(s);
    }));
    let env = Env::new("stop", &fake.url(), short());
    let mut lines = env.send("", MESSAGE);
    let conversation = lines.next().unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(lines.next().unwrap()["text"], "part");

    let started = Instant::now();
    let stopped = env.connect(&format!("chat-stop {conversation}")).rest();
    assert_eq!(stopped, vec![json!({ "stopped": true })]);
    let rest = lines.rest();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(rest, vec![json!({ "event": "done", "stopped": true })]);
    assert_eq!(
        assistant_text(&env.get(&conversation)).as_deref(),
        Some("part")
    );
    // Nothing runs any more.
    let again = env.connect(&format!("chat-stop {conversation}")).rest();
    assert_eq!(again, vec![json!({ "stopped": false })]);
}

#[test]
fn a_slow_but_steady_answer_is_kept_whole_and_a_silent_one_is_cut() {
    let fake = Fake::start(Arc::new(|n, s| {
        head(s);
        if n == 0 {
            // Longer than the first-word deadline in all, a chunk well inside every deadline.
            for i in 0..8 {
                chunk(s, &format!("{i} "));
                thread::sleep(Duration::from_millis(250));
            }
            done(s);
        } else {
            chunk(s, "some words");
            hold(s);
        }
    }));
    let deadlines = Deadlines {
        connect: Duration::from_secs(5),
        first_word: Duration::from_millis(600),
        silence: Duration::from_millis(600),
    };
    let env = Env::new("slow", &fake.url(), deadlines);
    let events = env.send("", MESSAGE).rest();
    let (conversation, text, last) = summary(&events);
    assert_eq!(last, json!({ "event": "done" }), "{events:?}");
    assert_eq!(text, "0 1 2 3 4 5 6 7 ");
    assert_eq!(
        assistant_text(&env.get(&conversation)).as_deref(),
        Some(text.as_str())
    );

    let events = env.send("", MESSAGE).rest();
    let (conversation, text, last) = summary(&events);
    assert_eq!(text, "some words");
    assert_eq!(last, json!({ "event": "error", "reason": "cut" }));
    assert_eq!(
        assistant_text(&env.get(&conversation)).as_deref(),
        Some("some words")
    );
}

#[test]
fn a_second_send_while_one_runs_is_busy() {
    let fake = Fake::start(Arc::new(|n, s| {
        head(s);
        chunk(s, "x");
        if n == 0 {
            hold(s);
        }
        done(s);
    }));
    let env = Env::new("busy", &fake.url(), short());
    let mut first = env.send("", MESSAGE);
    let conversation = first.next().unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    first.next().unwrap();
    assert_eq!(reason(&env.send("", "second").rest()), "busy");
    assert_eq!(
        env.connect("chat-delete all").rest(),
        vec![json!({ "error": "busy" })]
    );
    assert_eq!(fake.bodies().len(), 1);
    env.connect(&format!("chat-stop {conversation}")).rest();
    first.rest();
    // The slot is free again.
    assert_eq!(
        env.send("", "third").rest().last().unwrap(),
        &json!({ "event": "done" })
    );
}

#[test]
fn every_request_starts_with_all_the_messages_of_the_one_before() {
    let fake = Fake::start(replying(&["answer"]));
    let env = Env::new("prefix", &fake.url(), short());
    let (conversation, _, _) = summary(&env.send("", "first").rest());
    env.send(&conversation, "second \"quoted\" ünï").rest();
    env.send(&conversation, "third").rest();
    let bodies = fake.bodies();
    assert_eq!(bodies.len(), 3);
    let messages = |b: &str| {
        let from = b.find("\"messages\":[").unwrap() + "\"messages\":[".len();
        let to = b.rfind("],\"stream\":true}").unwrap();
        b[from..to].to_string()
    };
    for pair in bodies.windows(2) {
        let (before, after) = (messages(&pair[0]), messages(&pair[1]));
        assert!(after.starts_with(&before));
        assert!(after[before.len()..].starts_with(','));
    }
    assert_eq!(
        messages(&bodies[2])
            .matches("\"role\":\"assistant\"")
            .count(),
        2
    );
}

#[test]
fn tool_records_replay_byte_for_byte() {
    let fake = Fake::start(replying(&["ok"]));
    let env = Env::new("tools", &fake.url(), short());
    let conversation = "0b9c1f0e-1111-4222-8333-444455556666";
    let arguments = "{\"z\": 1,  \"a\" :[3,2],\n \"m\":\"é\"}";
    let mut sink = FileSink::new(&env.data);
    let turn = |time: &str, role: &str, text: &str| {
        serde_json::from_value::<Record>(json!({
            "kind": "turn", "tool": "chat", "session": conversation, "time": time,
            "folder": "", "branch": "", "role": role, "text": text,
        }))
        .unwrap()
    };
    for record in [
        turn("2026-01-01T00:00:00Z", "user", "what do you know"),
        Record::ToolCall(ToolCall {
            tool: "chat".into(),
            session: conversation.into(),
            time: "2026-01-01T00:00:01Z".into(),
            call_id: "call_9".into(),
            name: "recall_facts".into(),
            arguments: arguments.into(),
        }),
        Record::ToolResult(ToolResult {
            tool: "chat".into(),
            session: conversation.into(),
            time: "2026-01-01T00:00:02Z".into(),
            call_id: "call_9".into(),
            text: "Error: memory is off".into(),
        }),
        turn(
            "2999-01-01T00:00:03Z",
            "assistant",
            "Nothing is remembered.",
        ),
    ] {
        sink.append(&record).unwrap();
    }
    sink.flush_session("chat", conversation).unwrap();

    let records = env.get(conversation);
    assert_eq!(records[1]["kind"], "toolcall");
    assert_eq!(records[1]["arguments"], arguments);
    assert_eq!(records[2]["kind"], "toolresult");

    // With memory off the new result is the planted one, so no new call comes after the turn.
    env.settings
        .apply(Update::parse(&json!({ "collection": { "enabled": false } }).to_string()).unwrap())
        .unwrap();
    let events = env.send(conversation, "and now?").rest();
    assert_eq!(events.last().unwrap(), &json!({ "event": "done" }));
    let body = &fake.bodies()[0];
    let escaped = serde_json::to_string(arguments).unwrap();
    let expected = format!(
        concat!(
            r#"{{"role":"user","content":"what do you know"}},"#,
            r#"{{"role":"assistant","content":null,"tool_calls":[{{"id":"call_9","type":"function","function":{{"name":"recall_facts","arguments":{}}}}}]}},"#,
            r#"{{"role":"tool","content":"Error: memory is off","tool_call_id":"call_9"}},"#,
            r#"{{"role":"assistant","content":"Nothing is remembered."}},"#,
            r#"{{"role":"user","content":"and now?"}}],"stream":true}}"#
        ),
        escaped
    );
    assert!(body.ends_with(&expected), "{body}");
    // Times only grow: the new turns come after the planted ones.
    let records = env.get(conversation);
    let times: Vec<&str> = records
        .iter()
        .map(|r| r["time"].as_str().unwrap())
        .collect();
    assert!(
        times.windows(2).all(|w| compare_times(w[0], w[1]).is_lt()),
        "{times:?}"
    );
    assert!(times[4].starts_with("2999-"), "{times:?}");
}

#[test]
fn list_names_a_conversation_after_its_first_message_and_delete_removes_it() {
    let fake = Fake::start(replying(&["ok"]));
    let env = Env::new("list", &fake.url(), short());
    let long = format!("{}{}", "é".repeat(59), "abcdefgh");
    let (conversation, _, _) = summary(&env.send("", &long).rest());
    let list = env.connect("chat-list").rest();
    let item = &list[0]["conversations"][0];
    assert_eq!(item["conversation"], conversation.as_str());
    assert_eq!(item["title"], format!("{}a", "é".repeat(59)));

    let deleted = env.connect(&format!("chat-delete {conversation}")).rest();
    assert_eq!(deleted, vec![json!({ "deleted": 1 })]);
    assert_eq!(
        env.connect("chat-list").rest(),
        vec![json!({ "conversations": [] })]
    );
    assert_eq!(
        env.get(&conversation),
        vec![json!({ "error": "unknown conversation" })]
    );

    env.send("", "a").rest();
    env.send("", "b").rest();
    assert_eq!(
        env.connect("chat-delete all").rest(),
        vec![json!({ "deleted": 2 })]
    );
}

#[test]
fn bad_requests_are_refused_plainly() {
    let fake = Fake::start(replying(&["ok"]));
    let env = Env::new("bad", &fake.url(), short());
    assert_eq!(
        reason(&env.connect("chat-send nonsense").rest()),
        "invalid request"
    );
    assert_eq!(reason(&env.send("", "  ").rest()), "empty message");
    assert_eq!(
        reason(&env.send("../x", "hi").rest()),
        "unknown conversation"
    );
    let unknown = "0b9c1f0e-1111-4222-8333-444455556666";
    assert_eq!(
        reason(&env.send(unknown, "hi").rest()),
        "unknown conversation"
    );

    // A long message is fine up to the cap of its own; more is refused without an echo.
    let events = env.send("", &"x".repeat(300_000)).rest();
    assert_eq!(events.last().unwrap(), &json!({ "event": "done" }));
    let line = format!("chat-send {{\"message\":\"{}\"}}", "y".repeat(1024 * 1024));
    let events = env.connect(&line).rest();
    assert_eq!(reason(&events), "request too large");
    assert!(!serde_json::to_string(&events).unwrap().contains("yyy"));
    // Other commands keep the small cap.
    let events = env
        .connect(&format!("status {}", "z".repeat(20_000)))
        .rest();
    assert_eq!(events, vec![json!({ "error": "request too long" })]);
    assert_eq!(fake.bodies().len(), 1);
}

#[test]
fn without_a_model_the_answer_is_no_model_and_nothing_is_stored() {
    let fake = Fake::start(replying(&["ok"]));
    let env = Env::new("nomodel", &fake.url(), short());
    let update = json!({ "model": { "remove_key": true } });
    env.settings
        .apply(Update::parse(&update.to_string()).unwrap())
        .unwrap();
    assert_eq!(reason(&env.send("", MESSAGE).rest()), "no model");
    assert_eq!(
        env.connect("chat-list").rest(),
        vec![json!({ "conversations": [] })]
    );
    assert!(fake.bodies().is_empty());
}

#[test]
fn chunks_without_text_are_skipped_and_a_finish_reason_ends_the_answer() {
    let fake = Fake::start(Arc::new(|_, s| {
        head(s);
        raw_chunk(
            s,
            &json!({ "choices": [{ "delta": { "role": "assistant" } }] }),
        );
        raw_chunk(s, &json!({ "choices": [{ "delta": { "content": "" } }] }));
        raw_chunk(
            s,
            &json!({ "choices": [{ "delta": { "reasoning_content": "thinking" } }] }),
        );
        raw_chunk(s, &json!({ "choices": [{ "delta": { "content": "Hi" } }] }));
        raw_chunk(s, &json!({ "choices": [], "usage": { "total_tokens": 3 } }));
        write!(s, ": a comment\n\ndata: not json\n\n").unwrap();
        raw_chunk(
            s,
            &json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }),
        );
        // The connection closes without `[DONE]`.
    }));
    let env = Env::new("odd-chunks", &fake.url(), short());
    let events = env.send("", MESSAGE).rest();
    let (conversation, text, last) = summary(&events);
    assert_eq!(text, "Hi");
    // One delta only: an empty `content` is no piece.
    let deltas: Vec<&Value> = events.iter().filter(|e| e["event"] == "delta").collect();
    assert_eq!(deltas, [&json!({ "event": "delta", "text": "Hi" })]);
    assert_eq!(last, json!({ "event": "done" }), "{events:?}");
    assert_eq!(
        assistant_text(&env.get(&conversation)).as_deref(),
        Some("Hi")
    );
}

#[test]
fn a_success_that_is_no_event_stream_is_an_error_not_an_answer() {
    let fake = Fake::start(Arc::new(|_, s| status(s, 200, r#"{"choices":[]}"#)));
    let env = Env::new("not-sse", &fake.url(), short());
    let events = env.send("", MESSAGE).rest();
    assert_eq!(reason(&events), "error 200");
    let (conversation, ..) = summary(&events);
    assert_eq!(assistant_text(&env.get(&conversation)), None);
}

#[test]
fn a_redirect_is_not_followed_and_the_key_stays_home() {
    let other = Fake::start(replying(&["leaked"]));
    let target = format!("{}/chat/completions", other.url());
    let fake = Fake::start(Arc::new(move |_, s| {
        write!(
            s,
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
    }));
    let env = Env::new("redirect", &fake.url(), short());
    assert_eq!(reason(&env.send("", MESSAGE).rest()), "error 307");
    assert!(other.bodies().is_empty());
    assert!(other.auth.lock().unwrap().is_empty());
}

#[test]
fn an_answer_past_the_text_cap_is_cut_and_keeps_what_came_before() {
    let fake = Fake::start(Arc::new(|_, s| {
        head(s);
        chunk(s, &"a".repeat(100));
        chunk(s, &"b".repeat(1024 * 1024));
        done(s);
    }));
    let env = Env::new("text-cap", &fake.url(), short());
    let events = env.send("", MESSAGE).rest();
    let (conversation, text, last) = summary(&events);
    assert_eq!(text, "a".repeat(100));
    assert_eq!(last, json!({ "event": "error", "reason": "cut" }));
    assert_eq!(assistant_text(&env.get(&conversation)), Some(text));
}

#[test]
fn a_stream_past_the_wire_cap_is_cut_even_before_any_text() {
    let fake = Fake::start(Arc::new(|_, s| {
        head(s);
        // A reasoning stream: events that carry no text, until the agent gives up.
        let thinking =
            json!({ "choices": [{ "delta": { "reasoning_content": "t".repeat(900) } }] });
        for _ in 0..1000 {
            if write!(s, "data: {thinking}\n\n").is_err() {
                return;
            }
        }
        done(s);
    }));
    let env = Env::build("wire-cap", &fake.url(), short(), Some(100_000));
    let events = env.send("", MESSAGE).rest();
    let (conversation, text, last) = summary(&events);
    assert_eq!(text, "");
    assert_eq!(
        last,
        json!({ "event": "error", "reason": "cut" }),
        "{events:?}"
    );
    assert_eq!(assistant_text(&env.get(&conversation)), None);
}

#[test]
fn a_client_that_goes_away_stops_the_answer_and_frees_the_slot() {
    let fake = Fake::start(Arc::new(|n, s| {
        head(s);
        if n == 0 {
            // Chunks without end: only a failed write to the client can stop this.
            while chunk(s, "w") {
                thread::sleep(Duration::from_millis(50));
            }
        } else {
            chunk(s, "back");
            done(s);
        }
    }));
    let env = Env::new("gone", &fake.url(), short());
    let mut lines = env.send("", MESSAGE);
    let conversation = lines.next().unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    lines.next().unwrap();
    drop(lines);
    let started = Instant::now();
    loop {
        let events = env.send(&conversation, "again").rest();
        if events.last().unwrap()["event"] == "done" {
            break;
        }
        assert_eq!(reason(&events), "busy");
        assert!(started.elapsed() < Duration::from_secs(5), "slot not freed");
        thread::sleep(Duration::from_millis(50));
    }
    // The part the client got is stored before the new turn.
    let records = env.get(&conversation);
    assert_eq!(records[1]["role"], "assistant");
    assert!(records[1]["text"].as_str().unwrap().starts_with('w'));
}

/// A `chat-send` line of `total` bytes with its newline.
fn line_of(total: usize) -> String {
    let (head, tail) = ("chat-send {\"message\":\"", "\"}");
    format!(
        "{head}{}{tail}",
        "m".repeat(total - 1 - head.len() - tail.len())
    )
}

#[test]
fn the_request_caps_hold_at_their_edges() {
    let fake = Fake::start(replying(&["ok"]));
    let env = Env::new("edges", &fake.url(), short());
    for total in [16 * 1024, 16 * 1024 + 1, 16 * 1024 + 2, 1024 * 1024] {
        let events = env.connect(&line_of(total)).rest();
        assert_eq!(
            events.last().unwrap()["event"],
            "done",
            "{total}: {events:?}"
        );
    }
    let events = env.connect(&line_of(1024 * 1024 + 1)).rest();
    assert_eq!(reason(&events), "request too large");
    // Other commands stay at 16 KiB: 16384 bytes with the newline pass, one more does not.
    let status = |total: usize| format!("status {}", " ".repeat(total - 1 - "status ".len()));
    assert_eq!(
        env.connect(&status(16 * 1024)).rest()[0]["status"],
        "running"
    );
    assert_eq!(
        env.connect(&status(16 * 1024 + 1)).rest(),
        vec![json!({ "error": "request too long" })]
    );
}

#[test]
fn the_slot_is_free_when_the_last_line_is_written() {
    let fake = Fake::start(replying(&["ok"]));
    let env = Env::new("slot-free", &fake.url(), short());
    let mut at_done = None;
    env.chat
        .send(&json!({ "message": MESSAGE }).to_string(), &mut |line| {
            if line.contains("\"done\"") {
                at_done = Some(env.chat.delete("all"));
            }
            true
        });
    assert_eq!(at_done.as_deref(), Some(r#"{"deleted":1}"#));
}

#[test]
fn a_blackholed_address_is_unreachable_not_a_timeout() {
    // Nothing answers there: the connect either fails at once or runs into its deadline, and
    // both are "unreachable".
    let deadlines = Deadlines {
        connect: Duration::from_millis(300),
        ..short()
    };
    let env = Env::new("blackhole", "https://10.255.255.1/v1", deadlines);
    let started = Instant::now();
    assert_eq!(reason(&env.send("", MESSAGE).rest()), "unreachable");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn sending_the_same_text_again_after_a_failed_answer_answers_the_stored_turn() {
    let fake = Fake::start(Arc::new(|n, s| {
        if n == 0 {
            status(s, 500, "{}");
        } else {
            head(s);
            chunk(s, "fine");
            done(s);
        }
    }));
    let env = Env::new("retry", &fake.url(), short());
    let (conversation, ..) = summary(&env.send("", MESSAGE).rest());
    assert_eq!(
        env.send(&conversation, MESSAGE).rest().last().unwrap(),
        &json!({ "event": "done" })
    );
    let records = env.get(&conversation);
    let roles: Vec<&str> = records
        .iter()
        .map(|r| r["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["user", "assistant"]);
    assert_eq!(fake.bodies()[1].matches(MESSAGE).count(), 1);
    // A different text is a new turn.
    env.send(&conversation, "and more").rest();
    assert_eq!(env.get(&conversation).len(), 4);
}

#[test]
fn a_server_without_chat_answers_each_command_in_its_own_shape() {
    let socket = std::env::temp_dir()
        .join(format!("construct-nochat-{}", std::process::id()))
        .join("construct")
        .join("agent.sock");
    let server = Server::bind(&socket).unwrap();
    thread::spawn(move || server.serve());
    let ask = |line: &str| {
        let mut s = UnixStream::connect(&socket).unwrap();
        writeln!(s, "{line}").unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        serde_json::from_str::<Value>(&out).unwrap()
    };
    assert_eq!(
        ask("chat-send {}"),
        json!({ "event": "error", "reason": "chat is not available" })
    );
    assert_eq!(
        ask("chat-list"),
        json!({ "error": "chat is not available" })
    );
}

#[test]
fn the_same_text_after_a_partial_answer_is_a_new_message() {
    let fake = Fake::start(Arc::new(|n, s| {
        head(s);
        chunk(s, if n == 0 { "half" } else { "whole" });
        if n == 0 {
            return;
        }
        done(s);
    }));
    let env = Env::new("retry-partial", &fake.url(), short());
    let (conversation, _, last) = summary(&env.send("", MESSAGE).rest());
    assert_eq!(last, json!({ "event": "error", "reason": "cut" }));
    env.send(&conversation, MESSAGE).rest();
    let records = env.get(&conversation);
    let roles: Vec<&str> = records
        .iter()
        .map(|r| r["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["user", "assistant", "user", "assistant"]);
    assert_eq!(fake.bodies()[1].matches(MESSAGE).count(), 2);
    assert!(fake.bodies()[1].contains("half"));
}

// The tool loop. A script answers request `n` with `turns[n]`: chunks, then `[DONE]`.

/// The fragment of a tool call in a chunk.
fn call_chunk(index: usize, id: Option<&str>, name: Option<&str>, arguments: &str) -> Value {
    let mut call = json!({ "index": index, "function": { "arguments": arguments } });
    if let Some(id) = id {
        call["id"] = json!(id);
        call["type"] = json!("function");
    }
    if let Some(name) = name {
        call["function"]["name"] = json!(name);
    }
    json!({ "choices": [{ "delta": { "tool_calls": [call] } }] })
}

fn finish(reason: &str) -> Value {
    json!({ "choices": [{ "delta": {}, "finish_reason": reason }] })
}

fn text_chunk(text: &str) -> Value {
    json!({ "choices": [{ "delta": { "content": text } }] })
}

/// One whole call in one chunk, then the end of the turn.
fn whole_call(id: &str, name: &str, arguments: &str) -> Vec<Value> {
    vec![
        call_chunk(0, Some(id), Some(name), arguments),
        finish("tool_calls"),
    ]
}

fn turns(turns: Vec<Vec<Value>>) -> Script {
    Arc::new(move |n, s| {
        head(s);
        for data in turns.get(n).into_iter().flatten() {
            raw_chunk(s, data);
        }
        done(s);
    })
}

/// The `messages` of a request body.
fn messages(body: &str) -> Vec<Value> {
    serde_json::from_str::<Value>(body).unwrap()["messages"]
        .as_array()
        .unwrap()
        .clone()
}

fn kinds(records: &[Value]) -> Vec<&str> {
    records
        .iter()
        .map(|r| r["kind"].as_str().unwrap())
        .collect()
}

/// A runner that answers `name` with the arguments it got, and notes the order of the calls.
fn noting(seen: &Arc<Mutex<Vec<String>>>) -> Runner {
    let seen = Arc::clone(seen);
    Arc::new(move |name, arguments| {
        seen.lock().unwrap().push(format!("{name} {arguments}"));
        if name == "fail" {
            Err("it broke".into())
        } else {
            Ok(format!("result of {name}"))
        }
    })
}

#[test]
fn a_call_is_run_and_its_result_goes_in_the_next_request() {
    let fake = Fake::start(turns(vec![
        whole_call("call_a", "lookup", r#"{"q":"blue door"}"#),
        vec![text_chunk("The "), text_chunk("answer"), finish("stop")],
    ]));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let env = Env::with_tools("one-call", &fake.url(), noting(&seen));
    let events = env.send("", MESSAGE).rest();
    let (conversation, text, last) = summary(&events);
    assert_eq!(last, json!({ "event": "done" }), "{events:?}");
    assert_eq!(text, "The answer");
    assert_eq!(*seen.lock().unwrap(), [r#"lookup {"q":"blue door"}"#]);

    let records = env.get(&conversation);
    assert_eq!(kinds(&records), ["turn", "toolcall", "toolresult", "turn"]);
    assert_eq!(records[1]["call_id"], "call_a");
    assert_eq!(records[1]["name"], "lookup");
    assert_eq!(records[2]["call_id"], "call_a");
    assert_eq!(records[2]["text"], "result of lookup");
    let times: Vec<&str> = records
        .iter()
        .map(|r| r["time"].as_str().unwrap())
        .collect();
    assert!(
        times.windows(2).all(|w| compare_times(w[0], w[1]).is_lt()),
        "{times:?}"
    );

    let bodies = fake.bodies();
    assert_eq!(bodies.len(), 2);
    // The tools are declared in both, and the second request holds the call and its result.
    assert!(bodies.iter().all(|b| b.contains("\"tools\"")));
    let sent = messages(&bodies[1]);
    assert_eq!(sent.len(), 4);
    assert_eq!(sent[2]["tool_calls"][0]["id"], "call_a");
    assert_eq!(sent[2]["tool_calls"][0]["function"]["name"], "lookup");
    assert_eq!(sent[3]["role"], "tool");
    assert_eq!(sent[3]["tool_call_id"], "call_a");
    assert_eq!(sent[3]["content"], "result of lookup");
    // The key and the message reach the client in no event.
    let all = serde_json::to_string(&events).unwrap();
    assert!(!all.contains(KEY) && !all.contains(MESSAGE), "{all}");
}

#[test]
fn two_calls_in_one_message_run_in_order_and_both_are_answered() {
    let fake = Fake::start(turns(vec![
        vec![
            call_chunk(0, Some("call_a"), Some("first"), "{}"),
            call_chunk(1, Some("call_b"), Some("fail"), "{\"x\":1}"),
            finish("tool_calls"),
        ],
        vec![text_chunk("ok"), finish("stop")],
    ]));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let env = Env::with_tools("two-calls", &fake.url(), noting(&seen));
    let events = env.send("", MESSAGE).rest();
    assert_eq!(events.last().unwrap(), &json!({ "event": "done" }));
    assert_eq!(*seen.lock().unwrap(), ["first {}", r#"fail {"x":1}"#]);
    let sent = messages(&fake.bodies()[1]);
    // The system instruction, the user, one assistant message with both calls, two results.
    assert_eq!(sent.len(), 5, "{sent:?}");
    let calls = sent[2]["tool_calls"].as_array().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["id"], "call_a");
    assert_eq!(calls[1]["id"], "call_b");
    assert_eq!(sent[3]["tool_call_id"], "call_a");
    assert_eq!(sent[3]["content"], "result of first");
    // A tool error is only a result.
    assert_eq!(sent[4]["tool_call_id"], "call_b");
    assert_eq!(sent[4]["content"], "Error: it broke");
}

#[test]
fn arguments_come_back_byte_for_byte() {
    let arguments = "{ \"z\":1,\n  \"a\" : [3,  2],\t\"m\":\"é\\n\" }";
    let fake = Fake::start(turns(vec![
        whole_call("call_a", "lookup", arguments),
        vec![text_chunk("ok"), finish("stop")],
    ]));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let env = Env::with_tools("odd-arguments", &fake.url(), noting(&seen));
    let events = env.send("", MESSAGE).rest();
    assert_eq!(events.last().unwrap(), &json!({ "event": "done" }));
    assert_eq!(*seen.lock().unwrap(), [format!("lookup {arguments}")]);
    let escaped = serde_json::to_string(arguments).unwrap();
    let expected = format!(r#""function":{{"name":"lookup","arguments":{escaped}}}"#);
    assert!(fake.bodies()[1].contains(&expected), "{}", fake.bodies()[1]);
}

#[test]
fn arguments_split_over_chunks_and_fragments_without_an_id_are_put_together() {
    let fake = Fake::start(turns(vec![
        vec![
            // The first fragment has the id and the name; the later ones only the index. The two
            // calls are interleaved.
            call_chunk(0, Some("call_a"), Some("lookup"), "{\"q\""),
            call_chunk(1, Some("call_b"), Some("other"), ""),
            call_chunk(0, None, None, ": \"blue"),
            call_chunk(1, None, None, "{ \"k\" :"),
            call_chunk(0, None, None, " door\" }"),
            call_chunk(1, None, None, " 1}"),
            finish("tool_calls"),
        ],
        vec![text_chunk("ok"), finish("stop")],
    ]));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let env = Env::with_tools("split", &fake.url(), noting(&seen));
    let events = env.send("", MESSAGE).rest();
    assert_eq!(events.last().unwrap(), &json!({ "event": "done" }));
    assert_eq!(
        *seen.lock().unwrap(),
        [r#"lookup {"q": "blue door" }"#, r#"other { "k" : 1}"#]
    );
    let sent = messages(&fake.bodies()[1]);
    assert_eq!(sent[2]["tool_calls"][0]["id"], "call_a");
    assert_eq!(sent[2]["tool_calls"][1]["id"], "call_b");
}

#[test]
fn a_chunk_with_only_usage_between_the_fragments_and_the_finish_changes_nothing() {
    let fake = Fake::start(turns(vec![
        vec![
            call_chunk(0, Some("call_a"), Some("lookup"), "{\"q\":"),
            json!({ "usage": { "total_tokens": 7 } }),
            call_chunk(0, None, None, "1}"),
            finish("tool_calls"),
        ],
        vec![text_chunk("fine"), finish("stop")],
    ]));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let env = Env::with_tools("usage", &fake.url(), noting(&seen));
    let events = env.send("", MESSAGE).rest();
    let (_, text, last) = summary(&events);
    assert_eq!(last, json!({ "event": "done" }), "{events:?}");
    assert_eq!(text, "fine");
    assert_eq!(*seen.lock().unwrap(), [r#"lookup {"q":1}"#]);
}

#[test]
fn null_lists_in_a_chunk_do_not_cost_the_text() {
    let fake = Fake::start(Arc::new(|_, s| {
        head(s);
        raw_chunk(
            s,
            &json!({ "choices": [{ "delta": { "content": "kept", "tool_calls": null } }] }),
        );
        // A chunk whose `choices` is `null` and an empty one, between text and the finish.
        raw_chunk(
            s,
            &json!({ "choices": null, "usage": { "total_tokens": 3 } }),
        );
        raw_chunk(s, &json!({ "choices": [], "usage": { "total_tokens": 3 } }));
        raw_chunk(s, &finish("stop"));
        done(s);
    }));
    let env = Env::new("null-lists", &fake.url(), short());
    let (_, text, last) = summary(&env.send("", MESSAGE).rest());
    assert_eq!(text, "kept");
    assert_eq!(last, json!({ "event": "done" }));
}

#[test]
fn a_finish_reason_ends_the_turn_while_the_connection_stays_open() {
    let fake = Fake::start(Arc::new(|n, s| {
        head(s);
        if n == 0 {
            for data in whole_call("call_a", "lookup", "{}") {
                raw_chunk(s, &data);
            }
            // No `[DONE]`, and the connection stays open.
            hold(s);
        } else {
            raw_chunk(s, &text_chunk("fine"));
            raw_chunk(s, &finish("stop"));
            hold(s);
        }
    }));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let env = Env::with_tools("open-connection", &fake.url(), noting(&seen));
    let at = Instant::now();
    let events = env.send("", MESSAGE).rest();
    assert_eq!(events.last().unwrap(), &json!({ "event": "done" }));
    assert!(at.elapsed() < Duration::from_secs(5), "{:?}", at.elapsed());
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn calls_of_a_turn_that_was_cut_short_are_not_kept() {
    let fake = Fake::start(turns(vec![vec![
        call_chunk(0, Some("call_a"), Some("lookup"), "{\"q\":\"bl"),
        finish("length"),
    ]]));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let env = Env::with_tools("length", &fake.url(), noting(&seen));
    let events = env.send("", MESSAGE).rest();
    let (conversation, _, last) = summary(&events);
    assert_eq!(last, json!({ "event": "error", "reason": "cut" }));
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(kinds(&env.get(&conversation)), ["turn"]);
}

#[test]
fn a_turn_whose_calls_may_not_be_whole_or_have_no_name_is_cut_and_runs_nothing() {
    // `finish_reason` other than `stop` and `tool_calls`, and a call with no name.
    for (n, chunks) in [
        vec![
            call_chunk(0, Some("call_a"), Some("lookup"), "{}"),
            finish("error"),
        ],
        vec![
            call_chunk(0, Some("call_a"), None, "{}"),
            finish("tool_calls"),
        ],
    ]
    .into_iter()
    .enumerate()
    {
        let fake = Fake::start(turns(vec![chunks]));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let env = Env::with_tools(&format!("not-whole-{n}"), &fake.url(), noting(&seen));
        let events = env.send("", MESSAGE).rest();
        let (conversation, _, last) = summary(&events);
        assert_eq!(last, json!({ "event": "error", "reason": "cut" }), "{n}");
        assert!(seen.lock().unwrap().is_empty());
        assert_eq!(kinds(&env.get(&conversation)), ["turn"]);
        assert_eq!(fake.bodies().len(), 1);
    }
}

#[test]
fn a_repeated_index_with_a_new_id_is_another_call_and_a_bare_call_gets_an_id() {
    let fake = Fake::start(turns(vec![
        vec![
            call_chunk(0, Some("call_a"), Some("lookup"), "{\"n\":1}"),
            call_chunk(0, Some("call_b"), Some("lookup"), "{\"n\":2}"),
            // No id, and no arguments at all.
            json!({ "choices": [{ "delta": { "tool_calls": [
                { "index": 1, "function": { "name": "now" } }
            ] } }] }),
            finish("tool_calls"),
        ],
        vec![text_chunk("ok"), finish("stop")],
    ]));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let env = Env::with_tools("same-index", &fake.url(), noting(&seen));
    let events = env.send("", MESSAGE).rest();
    assert_eq!(events.last().unwrap(), &json!({ "event": "done" }));
    assert_eq!(
        *seen.lock().unwrap(),
        [r#"lookup {"n":1}"#, r#"lookup {"n":2}"#, "now {}"]
    );
    let sent = messages(&fake.bodies()[1]);
    let calls = sent[2]["tool_calls"].as_array().unwrap();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0]["id"], "call_a");
    assert_eq!(calls[1]["id"], "call_b");
    let generated = calls[2]["id"].as_str().unwrap();
    assert!(generated.starts_with("call_") && generated.len() > 10);
    assert_eq!(calls[2]["function"]["arguments"], "{}");
    assert_eq!(sent[5]["tool_call_id"], generated);
}

#[test]
fn an_unknown_tool_is_a_result_and_the_model_goes_on() {
    let fake = Fake::start(turns(vec![
        whole_call("call_a", "nothing_like_it", "{}"),
        vec![text_chunk("sorry"), finish("stop")],
    ]));
    // No tools are given to this chat.
    let env = Env::new("unknown-tool", &fake.url(), short());
    let events = env.send("", MESSAGE).rest();
    let (conversation, text, last) = summary(&events);
    assert_eq!(last, json!({ "event": "done" }));
    assert_eq!(text, "sorry");
    let records = env.get(&conversation);
    assert_eq!(records[2]["text"], "Error: unknown tool nothing_like_it");
    let sent = messages(&fake.bodies()[1]);
    assert_eq!(sent[3]["content"], "Error: unknown tool nothing_like_it");
}

#[test]
fn an_unknown_name_is_a_result_even_when_the_chat_has_tools() {
    let fake = Fake::start(turns(vec![
        whole_call("call_a", "nothing_like_it", "{}"),
        vec![text_chunk("sorry"), finish("stop")],
    ]));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let env = Env::with_tools("unknown-listed", &fake.url(), noting(&seen));
    let events = env.send("", MESSAGE).rest();
    let (conversation, _, last) = summary(&events);
    assert_eq!(last, json!({ "event": "done" }));
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(
        env.get(&conversation)[2]["text"],
        "Error: unknown tool nothing_like_it"
    );
}

#[test]
fn thirty_calls_in_a_row_all_run_in_order_and_the_answer_ends_normally() {
    // The model asks for one call in each of its first thirty turns, then answers.
    let fake = Fake::start(Arc::new(|n, s| {
        head(s);
        if n < 30 {
            for data in whole_call(&format!("call_{n}"), "lookup", &format!(r#"{{"q":"{n}"}}"#)) {
                raw_chunk(s, &data);
            }
        } else {
            raw_chunk(s, &text_chunk("All done."));
            raw_chunk(s, &finish("stop"));
        }
        done(s);
    }));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let env = Env::with_tools("thirty", &fake.url(), noting(&seen));
    let events = env.send("", MESSAGE).rest();
    let (conversation, text, last) = summary(&events);
    assert_eq!(last, json!({ "event": "done" }), "{events:?}");
    assert_eq!(text, "All done.");
    let expected: Vec<String> = (0..30)
        .map(|n| format!(r#"lookup {{"q":"{n}"}}"#))
        .collect();
    assert_eq!(*seen.lock().unwrap(), expected);
    assert_eq!(fake.bodies().len(), 31);
    let records = env.get(&conversation);
    assert_eq!(
        records.iter().filter(|r| r["kind"] == "toolcall").count(),
        30
    );
    assert_eq!(
        records.iter().filter(|r| r["kind"] == "toolresult").count(),
        30
    );
    assert_eq!(assistant_text(&records).as_deref(), Some("All done."));
}

#[test]
fn ten_calls_in_one_turn_all_run() {
    let fake = Fake::start(turns(vec![
        {
            let mut chunks: Vec<Value> = (0..10)
                .map(|i| call_chunk(i, Some(&format!("c{i}")), Some("lookup"), "{}"))
                .collect();
            chunks.push(finish("tool_calls"));
            chunks
        },
        vec![text_chunk("Done."), finish("stop")],
    ]));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let env = Env::with_tools("ten-at-once", &fake.url(), noting(&seen));
    let events = env.send("", MESSAGE).rest();
    let (_, text, last) = summary(&events);
    assert_eq!(last, json!({ "event": "done" }), "{events:?}");
    assert_eq!(text, "Done.");
    assert_eq!(seen.lock().unwrap().len(), 10);
    assert_eq!(fake.bodies().len(), 2);
}

#[test]
fn stop_during_the_twenty_fifth_call_ends_within_two_seconds_and_answers_every_call() {
    // One quick call in each of the first 24 turns, a slow one in the 25th, then an answer.
    let fake = Fake::start(Arc::new(|n, s| {
        head(s);
        if n < 25 {
            let name = if n == 24 { "slow" } else { "lookup" };
            for data in whole_call(&format!("call_{n}"), name, "{}") {
                raw_chunk(s, &data);
            }
        } else {
            raw_chunk(s, &text_chunk("after the stop"));
            raw_chunk(s, &finish("stop"));
        }
        done(s);
    }));
    let (runner, started_rx, release_tx) = blocking(Some("slow"));
    let env = Env::with_tools("stop-25th", &fake.url(), runner);
    let mut lines = env.send("", MESSAGE);
    let conversation = lines.next().unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();

    let at = Instant::now();
    let stopped = env.connect(&format!("chat-stop {conversation}")).rest();
    assert_eq!(stopped, vec![json!({ "stopped": true })]);
    let rest = lines.rest();
    assert!(at.elapsed() < Duration::from_secs(2), "{:?}", at.elapsed());
    assert_eq!(
        rest.last().unwrap(),
        &json!({ "event": "done", "stopped": true })
    );
    let _ = release_tx.send(());

    let records = env.get(&conversation);
    assert_eq!(
        records.iter().filter(|r| r["kind"] == "toolcall").count(),
        25
    );
    let results: Vec<&Value> = records
        .iter()
        .filter(|r| r["kind"] == "toolresult")
        .collect();
    assert_eq!(results.len(), 25);
    assert_eq!(results[24]["text"], "stopped by the user");

    // The next request has a result for every call.
    let events = env.send(&conversation, "and then?").rest();
    assert_eq!(events.last().unwrap(), &json!({ "event": "done" }));
    let bodies = fake.bodies();
    let sent = messages(bodies.last().unwrap());
    let answers = sent.iter().filter(|m| m["role"] == "tool").count();
    assert_eq!(answers, 25);
}

#[test]
fn stop_while_a_call_runs_answers_every_call_and_ends_within_two_seconds() {
    let fake = Fake::start(turns(vec![
        vec![
            call_chunk(0, Some("call_a"), Some("slow"), "{}"),
            call_chunk(1, Some("call_b"), Some("slow"), "{}"),
            finish("tool_calls"),
        ],
        vec![text_chunk("after the stop"), finish("stop")],
    ]));
    let (runner, started_rx, release_tx) = blocking(Some("slow"));
    let env = Env::with_tools("stop-call", &fake.url(), runner);
    let mut lines = env.send("", MESSAGE);
    let conversation = lines.next().unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();

    let at = Instant::now();
    let stopped = env.connect(&format!("chat-stop {conversation}")).rest();
    assert_eq!(stopped, vec![json!({ "stopped": true })]);
    let rest = lines.rest();
    assert!(at.elapsed() < Duration::from_secs(2), "{:?}", at.elapsed());
    // Only the call that started is announced.
    assert_eq!(
        rest,
        vec![
            json!({ "event": "tool", "line": "slow" }),
            json!({ "event": "done", "stopped": true })
        ]
    );
    let _ = release_tx.send(());

    let records = env.get(&conversation);
    assert_eq!(
        kinds(&records),
        ["turn", "toolcall", "toolcall", "toolresult", "toolresult"]
    );
    assert_eq!(records[3]["text"], "stopped by the user");
    assert_eq!(records[4]["text"], "stopped by the user");

    // The next request answers every call.
    let events = env.send(&conversation, "and then?").rest();
    assert_eq!(events.last().unwrap(), &json!({ "event": "done" }));
    let sent = messages(&fake.bodies()[1]);
    let answers: Vec<&Value> = sent.iter().filter(|m| m["role"] == "tool").collect();
    assert_eq!(answers.len(), 2);
    assert!(
        answers
            .iter()
            .all(|m| m["content"] == "stopped by the user")
    );
}

#[test]
fn stop_during_a_later_call_keeps_the_real_results_before_it() {
    let fake = Fake::start(turns(vec![vec![
        call_chunk(0, Some("call_a"), Some("quick"), "{}"),
        call_chunk(1, Some("call_b"), Some("slow"), "{}"),
        call_chunk(2, Some("call_c"), Some("slow"), "{}"),
        finish("tool_calls"),
    ]]));
    let (runner, started_rx, release_tx) = blocking(Some("slow"));
    let env = Env::with_tools("stop-later", &fake.url(), runner);
    let mut lines = env.send("", MESSAGE);
    let conversation = lines.next().unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    env.connect(&format!("chat-stop {conversation}")).rest();
    lines.rest();
    let _ = release_tx.send(());
    let results: Vec<String> = env
        .get(&conversation)
        .iter()
        .filter(|r| r["kind"] == "toolresult")
        .map(|r| {
            format!(
                "{} {}",
                r["call_id"].as_str().unwrap(),
                r["text"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        results,
        [
            "call_a quick done",
            "call_b stopped by the user",
            "call_c stopped by the user"
        ]
    );
}

/// The `tools` array of a request body as written, which ends the body after `stream`.
fn tools_text(body: &str) -> &str {
    let (_, rest) = body.split_once(r#""stream":true,"tools":"#).unwrap();
    rest.strip_suffix('}').unwrap()
}

#[test]
fn the_tools_array_is_the_same_bytes_in_every_request_of_a_conversation() {
    let fake = Fake::start(turns(vec![
        whole_call("call_a", "lookup", r#"{"q":"one"}"#),
        whole_call("call_b", "other", r#"{"q":"two"}"#),
        vec![text_chunk("done"), finish("stop")],
    ]));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let env = Env::with_tools("tools-bytes", &fake.url(), noting(&seen));
    let events = env.send("", MESSAGE).rest();
    assert_eq!(events.last().unwrap(), &json!({ "event": "done" }));
    let bodies = fake.bodies();
    assert_eq!(bodies.len(), 3);
    let expected = |name: &str| {
        format!(
            r#"{{"type":"function","function":{{"name":"{name}","description":"A tool for the tests.","parameters":{{"type":"object","properties":{{"q":{{"type":"string"}}}}}}}}}}"#
        )
    };
    let listed: Vec<String> = NAMES.iter().map(|name| expected(name)).collect();
    let first = tools_text(&bodies[0]);
    assert_eq!(first, format!("[{}]", listed.join(",")));
    assert_eq!(tools_text(&bodies[1]), first);
    assert_eq!(tools_text(&bodies[2]), first);
}

#[test]
fn without_tools_the_body_has_no_tools_field() {
    let fake = Fake::start(replying(&["ok"]));
    let env = Env::new("no-tools-field", &fake.url(), short());
    env.send("", MESSAGE).rest();
    let body = &fake.bodies()[0];
    assert!(body.ends_with(r#""stream":true}"#), "{body}");
    assert!(!body.contains("\"tools\""), "{body}");
}

#[test]
fn a_blocked_tool_holds_neither_the_agent_nor_the_stop() {
    let fake = Fake::start(turns(vec![
        whole_call("call_a", "slow", "{}"),
        vec![text_chunk("after"), finish("stop")],
    ]));
    let (runner, started_rx, release_tx) = blocking(None);
    let env = Env::with_tools("blocked-tool", &fake.url(), runner);
    let mut lines = env.send("", MESSAGE);
    let conversation = lines.next().unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();

    let at = Instant::now();
    assert_eq!(env.connect("status").rest()[0]["status"], "running");
    assert!(env.connect("chat-list").rest()[0]["conversations"].is_array());
    assert!(at.elapsed() < Duration::from_secs(2), "{:?}", at.elapsed());

    let at = Instant::now();
    assert_eq!(
        env.connect(&format!("chat-stop {conversation}")).rest(),
        vec![json!({ "stopped": true })]
    );
    assert_eq!(
        lines.rest(),
        vec![
            json!({ "event": "tool", "line": "slow" }),
            json!({ "event": "done", "stopped": true })
        ]
    );
    assert!(at.elapsed() < Duration::from_secs(2), "{:?}", at.elapsed());

    // The abandoned call still holds its thread, and the next answer does not wait for it.
    let at = Instant::now();
    let events = env.send(&conversation, "and then?").rest();
    assert_eq!(
        events.last().unwrap(),
        &json!({ "event": "done" }),
        "{events:?}"
    );
    assert!(at.elapsed() < Duration::from_secs(2), "{:?}", at.elapsed());
    let _ = release_tx.send(());
}

#[test]
fn a_result_of_100_kib_is_cut_at_64_kib_with_the_note() {
    let fake = Fake::start(turns(vec![
        whole_call("call_a", "lookup", "{}"),
        vec![text_chunk("ok"), finish("stop")],
    ]));
    let runner: Runner = Arc::new(|_, _| Ok(format!("a{}", "é".repeat(50 * 1024))));
    let env = Env::with_tools("long-result", &fake.url(), runner);
    let events = env.send("", MESSAGE).rest();
    assert_eq!(events.last().unwrap(), &json!({ "event": "done" }));
    let sent = messages(&fake.bodies()[1]);
    let result = sent.iter().find(|m| m["role"] == "tool").unwrap()["content"]
        .as_str()
        .unwrap();
    // 100 KiB of two-byte characters: a single-byte start puts the room for the text inside a character, so the cut falls back one byte
    // to a character boundary, and the whole result is under 64 KiB.
    assert_eq!(result.len(), 64 * 1024 - 1);
    assert!(result.ends_with(" [cut]"));
    assert!(result.starts_with("aéé"));
}

const FILE_TEXT: &str = "the-text-of-the-file";

/// A conversation in which the model reads `notes.txt` between two pieces of text.
fn reading_script() -> Script {
    turns(vec![
        vec![
            text_chunk("Let me look. "),
            call_chunk(
                0,
                Some("call_r"),
                Some("lookup"),
                &json!({ "q": "notes.txt", "content": FILE_TEXT }).to_string(),
            ),
            finish("tool_calls"),
        ],
        vec![text_chunk("Found it."), finish("stop")],
    ])
}

#[test]
fn a_call_gives_a_tool_event_between_the_deltas() {
    let fake = Fake::start(reading_script());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let env = Env::with_tools("tool-event", &fake.url(), noting(&seen));
    let events = env.send("", MESSAGE).rest();
    let names: Vec<&str> = events
        .iter()
        .map(|e| e["event"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["start", "delta", "tool", "delta", "done"],
        "{events:?}"
    );
    assert_eq!(
        events[2],
        json!({ "event": "tool", "line": "Read notes.txt" })
    );
    // The line is the short form: the other arguments reach no event.
    let all = serde_json::to_string(&events).unwrap();
    assert!(!all.contains(FILE_TEXT), "{all}");
}

#[test]
fn chat_get_has_the_line_in_its_place_and_the_same_after_a_restart() {
    let fake = Fake::start(reading_script());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let env = Env::with_tools("tool-get", &fake.url(), noting(&seen));
    let (conversation, _, _) = summary(&env.send("", MESSAGE).rest());

    let entries = env.get_all(&conversation);
    let shape: Vec<(&str, &str)> = entries
        .iter()
        .map(|e| {
            (
                e["kind"].as_str().unwrap(),
                e["role"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        shape,
        [
            ("turn", "user"),
            ("turn", "assistant"),
            ("toolcall", ""),
            ("turn", "tool"),
            ("toolresult", ""),
            ("turn", "assistant"),
        ]
    );
    assert_eq!(entries[3]["text"], "Read notes.txt");
    assert!(
        !serde_json::to_string(&entries[3])
            .unwrap()
            .contains(FILE_TEXT)
    );

    // A new chat over the same files, as after a restart of the agent.
    let again = Chat::new(
        env.settings.clone(),
        &env.data,
        env.runtime_handle(),
        short(),
    )
    .unwrap()
    .with_tools(registry(noting(&seen)));
    let restarted: Vec<Value> = again
        .get(&conversation)
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(restarted, entries);
}

#[test]
fn the_facts_call_gives_neither_an_event_nor_a_line() {
    let fake = Fake::start(replying(&["ok"]));
    let env = Env::new("facts-hidden", &fake.url(), short());
    // With memory off every answer stores the facts call and its result.
    env.settings
        .apply(Update::parse(&json!({ "collection": { "enabled": false } }).to_string()).unwrap())
        .unwrap();
    let events = env.send("", MESSAGE).rest();
    assert!(events.iter().all(|e| e["event"] != "tool"), "{events:?}");
    let (conversation, _, _) = summary(&events);
    let entries = env.get_all(&conversation);
    assert!(
        entries
            .iter()
            .any(|e| e["kind"] == "toolcall" && e["name"] == "recall_facts"),
        "{entries:?}"
    );
    assert!(entries.iter().all(|e| e["role"] != "tool"), "{entries:?}");
}
