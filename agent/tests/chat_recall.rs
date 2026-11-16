#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too
//! The facts of the person as a `recall_facts` call in the conversation, against a fake Construct
//! (a local port with a canned profile) and a fake provider that records its requests.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use connectors::compare_times;
use construct_agent::chat::{Chat, Deadlines, Link};
use construct_agent::host::Connection;
use construct_agent::ipc::Server;
use construct_agent::settings::{Settings, Update};
use serde_json::{Value, json};

mod common;

use common::{chunk, done, head, read_request, status};

const TOKEN: &str = "construct-token-5e1a";

/// What the fake Construct does: the status and body of its answer, and how long it waits first.
#[derive(Clone)]
struct Profile {
    status: u16,
    body: String,
    delay: Duration,
}

fn profile(facts: &[(&str, &str, &str, &str)]) -> Profile {
    let mut categories: Vec<Value> = Vec::new();
    for (kind, property, value, at) in facts {
        let fact = json!({ "fact_id": "f", "property": property, "value": value, "stored_at": at });
        match categories.iter_mut().find(|c| c["entity_kind"] == *kind) {
            Some(c) => c["facts"].as_array_mut().unwrap().push(fact),
            None => categories.push(json!({ "entity_kind": kind, "facts": [fact] })),
        }
    }
    Profile {
        status: 200,
        body: json!({ "subject_id": "s", "categories": categories }).to_string(),
        delay: Duration::ZERO,
    }
}

struct FakeConstruct {
    addr: SocketAddr,
    answer: Arc<Mutex<Profile>>,
    auth: Arc<Mutex<Vec<String>>>,
}

impl FakeConstruct {
    fn reads(&self) -> usize {
        self.auth.lock().unwrap().len()
    }

    fn start(first: Profile) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let answer = Arc::new(Mutex::new(first));
        let auth = Arc::new(Mutex::new(Vec::new()));
        let (a, h) = (Arc::clone(&answer), Arc::clone(&auth));
        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let (a, h) = (Arc::clone(&a), Arc::clone(&h));
                thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut request_line = String::new();
                    reader.read_line(&mut request_line).unwrap();
                    loop {
                        let mut line = String::new();
                        reader.read_line(&mut line).unwrap();
                        if let Some((k, v)) = line.split_once(':')
                            && k.eq_ignore_ascii_case("authorization")
                        {
                            h.lock().unwrap().push(v.trim().to_string());
                        }
                        if line.trim().is_empty() {
                            break;
                        }
                    }
                    assert!(
                        request_line.starts_with("GET /construct/v1/subjects/"),
                        "{request_line}"
                    );
                    assert!(request_line.contains("/profile"), "{request_line}");
                    let p = a.lock().unwrap().clone();
                    thread::sleep(p.delay);
                    status(&mut stream, p.status, &p.body);
                });
            }
        });
        Self { addr, answer, auth }
    }

    fn set(&self, p: Profile) {
        *self.answer.lock().unwrap() = p;
    }
}

struct Provider {
    url: String,
    bodies: Arc<Mutex<Vec<String>>>,
}

impl Provider {
    fn start() -> Self {
        Self::start_failing(0)
    }

    /// A provider that answers the first `failures` requests with a 500.
    fn start_failing(failures: usize) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let b = Arc::clone(&bodies);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream: TcpStream = stream.unwrap();
                let (body, _) = read_request(&mut stream);
                let n = {
                    let mut bodies = b.lock().unwrap();
                    bodies.push(body);
                    bodies.len()
                };
                if n <= failures {
                    status(&mut stream, 500, "{}");
                    continue;
                }
                head(&mut stream);
                chunk(&mut stream, "ok");
                done(&mut stream);
            }
        });
        Self {
            url: format!("http://127.0.0.1:{port}/v1"),
            bodies,
        }
    }

    /// The messages of each request as the bytes of the body, one text per message (the body
    /// is never parsed: key order and spacing are part of what is compared).
    fn requests(&self) -> Vec<Vec<String>> {
        self.bodies
            .lock()
            .unwrap()
            .iter()
            .map(|b| {
                let inner = b
                    .strip_prefix(r#"{"model":"m","messages":["#)
                    .and_then(|b| b.strip_suffix(r#"],"stream":true}"#))
                    .unwrap();
                let mut parts = inner.split(r#",{"role":"#);
                let mut out = vec![parts.next().unwrap().to_string()];
                out.extend(parts.map(|p| format!("{{\"role\":{p}")));
                out
            })
            .collect()
    }
}

struct Env {
    socket: PathBuf,
    settings: Settings,
    _rt: tokio::runtime::Runtime,
}

impl Env {
    fn new(name: &str, provider: &Provider, construct: Option<&FakeConstruct>) -> Self {
        let base =
            std::env::temp_dir().join(format!("construct-recall-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let socket = base.join("run").join("construct").join("agent.sock");
        let settings = Settings::open(&base.join("config")).unwrap();
        let update = json!({ "model": { "base_url": provider.url, "model": "m", "api_key": "k" } });
        settings
            .apply(Update::parse(&update.to_string()).unwrap())
            .unwrap();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let link = Link::default();
        if let Some(c) = construct {
            link.set(Connection {
                port: c.addr.port(),
                token: TOKEN.into(),
            });
        }
        let deadlines = Deadlines {
            connect: Duration::from_secs(5),
            first_word: Duration::from_secs(10),
            silence: Duration::from_secs(10),
        };
        let chat = Chat::new(
            settings.clone(),
            &base.join("data"),
            rt.handle().clone(),
            deadlines,
        )
        .unwrap()
        .with_link(link);
        let server = Server::bind(&socket)
            .unwrap()
            .with_settings(settings.clone())
            .with_chat(chat);
        thread::spawn(move || server.serve());
        Self {
            socket,
            settings,
            _rt: rt,
        }
    }

    fn connect(&self, request: &str) -> Vec<Value> {
        let mut s = UnixStream::connect(&self.socket).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        s.write_all(request.as_bytes()).unwrap();
        s.write_all(b"\n").unwrap();
        let mut text = String::new();
        s.read_to_string(&mut text).unwrap();
        text.lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    /// Send one message; the conversation id and the events.
    fn send(&self, conversation: &str, message: &str) -> (String, Vec<Value>) {
        let events = self.connect(&format!(
            "chat-send {}",
            json!({ "conversation": conversation, "message": message })
        ));
        assert_eq!(
            events.last().unwrap(),
            &json!({ "event": "done" }),
            "{events:?}"
        );
        let id = events[0]["conversation"].as_str().unwrap().to_string();
        (id, events)
    }

    fn get(&self, conversation: &str) -> Vec<Value> {
        self.connect(&format!("chat-get {conversation}"))
    }

    fn collection(&self, enabled: bool) {
        let update = json!({ "collection": { "enabled": enabled } });
        self.settings
            .apply(Update::parse(&update.to_string()).unwrap())
            .unwrap();
    }
}

fn ann() -> Profile {
    profile(&[
        ("skill", "skills", "Rust", "2026-01-02T00:00:00Z"),
        ("identity", "name", "Ann", "2026-01-01T00:00:00Z"),
    ])
}

const ANN_TEXT: &str = "These are the facts known about the user. This list replaces all earlier lists of facts.\\nidentity.name: Ann\\nskill.skills: Rust";

fn is_tool_pair(request: &[String], at: usize) -> bool {
    request[at].contains(r#""role":"assistant","content":null,"tool_calls":[{"id":"call_"#)
        && request[at].contains(r#""name":"recall_facts","arguments":"{}""#)
        && request[at + 1].starts_with(r#"{"role":"tool","content":"#)
}

#[test]
fn the_first_request_has_the_call_and_the_facts_and_a_second_turn_adds_nothing() {
    let construct = FakeConstruct::start(ann());
    let provider = Provider::start();
    let env = Env::new("first", &provider, Some(&construct));
    let (id, _) = env.send("", "hello");
    env.send(&id, "and again");

    let requests = provider.requests();
    let first = &requests[0];
    assert_eq!(first.len(), 4);
    assert!(first[0].starts_with(r#"{"role":"system""#));
    assert_eq!(first[1], r#"{"role":"user","content":"hello"}"#);
    assert!(is_tool_pair(first, 2), "{first:?}");
    assert!(first[3].contains(ANN_TEXT), "{}", first[3]);
    assert!(first[3].contains(r#""tool_call_id":"call_"#));

    // The same profile adds nothing, and every earlier message is the same JSON text.
    let second = &requests[1];
    assert_eq!(second.len(), 4 + 2);
    assert_eq!(&second[..4], &first[..]);
    assert_eq!(second[4], r#"{"role":"assistant","content":"ok"}"#);
    assert_eq!(second[5], r#"{"role":"user","content":"and again"}"#);
    assert_eq!(construct.auth.lock().unwrap()[0], format!("Bearer {TOKEN}"));
}

#[test]
fn a_changed_or_removed_fact_adds_a_new_call_after_the_earlier_messages() {
    let construct = FakeConstruct::start(ann());
    let provider = Provider::start();
    let env = Env::new("changed", &provider, Some(&construct));
    let (id, _) = env.send("", "one");
    construct.set(profile(&[
        ("identity", "name", "Ann", "2026-01-01T00:00:00Z"),
        ("skill", "skills", "Rust", "2026-01-02T00:00:00Z"),
        ("role", "title", "Engineer", "2026-01-03T00:00:00Z"),
    ]));
    env.send(&id, "two");
    // A removed fact changes no `stored_at`, only the text.
    construct.set(profile(&[(
        "identity",
        "name",
        "Ann",
        "2026-01-01T00:00:00Z",
    )]));
    env.send(&id, "three");

    let requests = provider.requests();
    let (r1, r2, r3) = (&requests[0], &requests[1], &requests[2]);
    assert_eq!(&r2[..4], &r1[..]);
    assert_eq!(r2[4], r#"{"role":"assistant","content":"ok"}"#);
    assert_eq!(r2[5], r#"{"role":"user","content":"two"}"#);
    assert!(is_tool_pair(r2, 6), "{r2:?}");
    assert!(r2[7].contains("role.title: Engineer"));
    assert_eq!(&r3[..8], &r2[..]);
    assert!(is_tool_pair(r3, 10), "{r3:?}");
    assert!(!r3[11].contains("skill.skills"));
    assert!(r3[11].contains("identity.name: Ann"));
}

#[test]
fn the_switch_gives_the_error_and_back_gives_the_facts_again() {
    let construct = FakeConstruct::start(ann());
    let provider = Provider::start();
    let env = Env::new("switch", &provider, Some(&construct));
    let (id, _) = env.send("", "one");
    env.collection(false);
    env.send(&id, "two");
    env.collection(true);
    env.send(&id, "three");

    let requests = provider.requests();
    assert!(
        requests[1][7].contains("Error: memory is off"),
        "{:?}",
        requests[1]
    );
    assert!(requests[2][11].contains(ANN_TEXT), "{:?}", requests[2]);
    assert_eq!(requests[2].len(), 12);
}

#[test]
fn an_unreachable_slow_or_empty_construct_gives_no_call_and_the_answer_arrives() {
    let provider = Provider::start();
    let slow = FakeConstruct::start(Profile {
        delay: Duration::from_secs(3),
        ..ann()
    });
    let empty = FakeConstruct::start(profile(&[]));
    let refused = FakeConstruct::start(Profile {
        status: 503,
        ..ann()
    });
    let dead = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        FakeConstruct {
            addr: SocketAddr::from(([127, 0, 0, 1], port)),
            answer: Arc::new(Mutex::new(ann())),
            auth: Arc::default(),
        }
    };
    let cases: [(&str, Option<&FakeConstruct>); 5] = [
        ("slow", Some(&slow)),
        ("empty", Some(&empty)),
        ("refused", Some(&refused)),
        ("dead", Some(&dead)),
        ("none", None),
    ];
    for (name, construct) in cases {
        let env = Env::new(name, &provider, construct);
        let started = std::time::Instant::now();
        let (id, events) = env.send("", "hello");
        assert!(started.elapsed() < Duration::from_millis(2500), "{name}");
        assert!(events.iter().any(|e| e["event"] == "delta"), "{name}");
        let records = env.get(&id);
        assert!(
            records.iter().all(|r| r["kind"] == "turn"),
            "{name}: {records:?}"
        );
        env.send(&id, "again");
    }
    for request in provider.requests() {
        assert!(
            request.iter().all(|m| !m.contains("tool_call")),
            "{request:?}"
        );
    }
}

#[test]
fn chat_get_marks_the_tool_records_and_the_times_grow() {
    let construct = FakeConstruct::start(ann());
    let provider = Provider::start();
    let env = Env::new("get", &provider, Some(&construct));
    let (id, _) = env.send("", "hello");
    let records = env.get(&id);
    let kinds: Vec<&str> = records
        .iter()
        .map(|r| r["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["turn", "toolcall", "toolresult", "turn"]);
    assert_eq!(records[0]["role"], "user");
    assert_eq!(records[1]["hidden"], true);
    assert_eq!(records[2]["hidden"], true);
    assert!(records[0].get("hidden").is_none());
    assert_eq!(records[1]["name"], "recall_facts");
    assert_eq!(records[1]["arguments"], "{}");
    assert_eq!(records[1]["call_id"], records[2]["call_id"]);
    let times: Vec<&str> = records
        .iter()
        .map(|r| r["time"].as_str().unwrap())
        .collect();
    assert!(
        times.windows(2).all(|w| compare_times(w[0], w[1]).is_lt()),
        "{times:?}"
    );
    // The stored call id is the one a later request replays.
    env.send(&id, "more");
    let call_id = records[1]["call_id"].as_str().unwrap();
    assert!(provider.requests()[1][2].contains(call_id));
}

#[test]
fn the_same_text_with_another_stored_at_adds_nothing_and_off_reads_nothing() {
    let construct = FakeConstruct::start(ann());
    let provider = Provider::start();
    let env = Env::new("stored-at", &provider, Some(&construct));
    let (id, _) = env.send("", "one");
    construct.set(profile(&[
        ("skill", "skills", "Rust", "2030-05-05T00:00:00Z"),
        ("identity", "name", "Ann", "2030-05-06T00:00:00Z"),
    ]));
    env.send(&id, "two");
    let requests = provider.requests();
    assert_eq!(requests[1].len(), 6, "{:?}", requests[1]);
    assert_eq!(&requests[1][..4], &requests[0][..]);

    let reads = construct.reads();
    env.collection(false);
    env.send(&id, "three");
    assert_eq!(
        construct.reads(),
        reads,
        "the profile is not read while memory is off"
    );
}

#[test]
fn memory_off_on_the_first_message_stores_the_error_result() {
    let construct = FakeConstruct::start(ann());
    let provider = Provider::start();
    let env = Env::new("off-first", &provider, Some(&construct));
    env.collection(false);
    let (id, _) = env.send("", "hello");
    let first = &provider.requests()[0];
    assert_eq!(first.len(), 4);
    assert!(first[3].contains("Error: memory is off"), "{first:?}");
    assert_eq!(construct.reads(), 0);
    assert_eq!(env.get(&id).len(), 4);
}

#[test]
fn a_failed_first_answer_with_facts_is_a_retry_not_a_second_message() {
    let construct = FakeConstruct::start(ann());
    let provider = Provider::start_failing(1);
    let env = Env::new("retry", &provider, Some(&construct));
    let failed = env.connect(&format!(
        "chat-send {}",
        json!({ "conversation": "", "message": "hello" })
    ));
    assert_eq!(failed.last().unwrap()["event"], "error", "{failed:?}");
    let id = failed[0]["conversation"].as_str().unwrap().to_string();
    env.send(&id, "hello");

    let requests = provider.requests();
    assert_eq!(
        requests[1], requests[0],
        "the retry sends the same messages"
    );
    let kinds: Vec<Value> = env.get(&id).iter().map(|r| r["kind"].clone()).collect();
    assert_eq!(kinds, ["turn", "toolcall", "toolresult", "turn"]);
}
