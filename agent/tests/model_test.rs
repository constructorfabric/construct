//! The model Test call against a fake provider on a local port.
#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use construct_agent::ipc::Server;
use construct_agent::model_test::{run, DEFAULT_MODEL};
use construct_agent::settings::{Model, Settings};
use serde_json::{json, Value};

const KEY: &str = "sk-test-key-4f9a";
const LONG: Duration = Duration::from_secs(10);

struct Captured {
    request_line: String,
    authorization: Option<String>,
    body: Value,
}

enum Reply {
    Status(u16, String),
    Redirect(String),
    Hang,
    /// Headers and a few bytes of a longer body, then nothing.
    Stall,
    /// Answer after a pause.
    Slow(Duration, u16, String),
}

struct Fake {
    addr: SocketAddr,
    seen: Arc<Mutex<Vec<Captured>>>,
}

impl Fake {
    /// Answer the n-th connection with the n-th reply (the last one repeats).
    fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        thread::spawn(move || {
            let mut held = Vec::new();
            for (n, stream) in listener.incoming().enumerate() {
                let mut stream = stream.unwrap();
                let captured = read_request(&mut stream);
                log.lock().unwrap().push(captured);
                match &replies[n.min(replies.len() - 1)] {
                    Reply::Status(status, body) => write_reply(&mut stream, *status, "", body),
                    Reply::Redirect(to) => {
                        write_reply(&mut stream, 302, &format!("Location: {to}\r\n"), "");
                    }
                    Reply::Hang => held.push(stream),
                    Reply::Stall => {
                        let _ = write!(
                            stream,
                            "HTTP/1.1 200 X\r\nContent-Length: 500\r\nConnection: close\r\n\r\n{{\"choi"
                        );
                        let _ = stream.flush();
                        held.push(stream);
                    }
                    Reply::Slow(pause, status, body) => {
                        thread::sleep(*pause);
                        write_reply(&mut stream, *status, "", body);
                    }
                }
            }
        });
        Self { addr, seen }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.addr.port())
    }

    fn requests(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

fn read_request(stream: &mut TcpStream) -> Captured {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    reader.read_line(&mut request_line).unwrap();
    let mut authorization = None;
    let mut length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let line = line.trim();
        if line.is_empty() {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            length = v.trim().parse().unwrap();
        }
        if lower.starts_with("authorization:") {
            authorization = line.split_once(':').map(|(_, v)| v.trim().to_string());
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    Captured {
        request_line: request_line.trim().to_string(),
        authorization,
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
    }
}

fn write_reply(stream: &mut TcpStream, status: u16, headers: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\n{headers}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn completion(text: &str) -> Reply {
    Reply::Status(
        200,
        json!({ "choices": [{ "message": { "role": "assistant", "content": text } }] }).to_string(),
    )
}

fn stored(base_url: &str) -> Model {
    Model {
        base_url: base_url.to_string(),
        model: "my-model".to_string(),
        api_key: Some(KEY.to_string()),
    }
}

fn call(fake: &Fake) -> Value {
    let line = run("", &stored(&fake.url()), LONG);
    assert!(!line.contains(KEY), "the key is in the answer: {line}");
    serde_json::from_str(&line).unwrap()
}

#[test]
fn works_with_the_models_text_and_the_request_has_the_key_and_ping() {
    let fake = Fake::start(vec![completion("pong")]);
    let answer = call(&fake);
    assert_eq!(answer, json!({ "result": "works", "text": "pong" }));

    let seen = fake.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].request_line, "POST /v1/chat/completions HTTP/1.1");
    assert_eq!(
        seen[0].authorization.as_deref(),
        Some(&*format!("Bearer {KEY}"))
    );
    assert_eq!(seen[0].body["model"], "my-model");
    assert_eq!(seen[0].body["max_completion_tokens"], 128);
    assert!(seen[0].body.get("max_tokens").is_none());
    let messages = seen[0].body["messages"].as_array().unwrap();
    assert!(messages
        .iter()
        .any(|m| m["role"] == "user" && m["content"] == "ping"));
    assert!(messages
        .iter()
        .any(|m| m["content"].as_str().unwrap().contains("pong")));
}

#[test]
fn an_empty_text_from_a_valid_200_still_works() {
    let fake = Fake::start(vec![completion("")]);
    assert_eq!(call(&fake), json!({ "result": "works", "text": "" }));
    let fake = Fake::start(vec![Reply::Status(
        200,
        json!({ "choices": [{ "message": { "content": null } }] }).to_string(),
    )]);
    assert_eq!(call(&fake), json!({ "result": "works", "text": "" }));
}

#[test]
fn a_200_that_is_not_a_completion_is_an_error_with_the_status() {
    let fake = Fake::start(vec![Reply::Status(200, "<html>hello</html>".to_string())]);
    assert_eq!(call(&fake), json!({ "result": "error", "status": 200 }));
}

#[test]
fn the_models_text_is_cut_and_never_holds_the_key() {
    let fake = Fake::start(vec![completion(&"a".repeat(5000))]);
    let answer = call(&fake);
    assert_eq!(answer["text"].as_str().unwrap().len(), 200);
    let fake = Fake::start(vec![completion(&format!("my key is {KEY}"))]);
    assert_eq!(
        call(&fake),
        json!({ "result": "works", "text": "my key is [hidden]" })
    );
    // A key that would straddle the cut is scrubbed before the text is cut.
    let straddle = format!("{}{KEY}{}", "a".repeat(195), "b".repeat(50));
    let fake = Fake::start(vec![completion(&straddle)]);
    let answer = call(&fake);
    assert!(
        !answer["text"].as_str().unwrap().contains("sk-"),
        "{answer}"
    );
}

#[test]
fn the_answer_that_is_read_is_capped() {
    let fake = Fake::start(vec![Reply::Status(200, "x".repeat(200 * 1024))]);
    assert_eq!(call(&fake), json!({ "result": "error", "status": 200 }));
}

#[test]
fn refused_key_missing_model_and_server_error_give_a_reason_not_the_body() {
    let echo = format!("{{\"error\":{{\"message\":\"Incorrect API key provided: {KEY}\"}}}}");
    for (status, want) in [
        (401, json!({ "result": "key refused" })),
        (403, json!({ "result": "key refused" })),
        (404, json!({ "result": "model not found" })),
        (500, json!({ "result": "error", "status": 500 })),
        (429, json!({ "result": "error", "status": 429 })),
    ] {
        let fake = Fake::start(vec![Reply::Status(status, echo.clone())]);
        assert_eq!(call(&fake), want, "status {status}");
    }
}

#[test]
fn a_400_about_max_completion_tokens_is_retried_once_with_max_tokens() {
    let fake = Fake::start(vec![
        Reply::Status(
            400,
            "{\"error\":\"unknown parameter max_completion_tokens\"}".to_string(),
        ),
        completion("pong"),
    ]);
    assert_eq!(call(&fake), json!({ "result": "works", "text": "pong" }));
    let seen = fake.seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert!(seen[0].body.get("max_completion_tokens").is_some());
    assert!(seen[1].body.get("max_completion_tokens").is_none());
    assert_eq!(seen[1].body["max_tokens"], 128);
    assert_eq!(
        seen[1].authorization.as_deref(),
        Some(&*format!("Bearer {KEY}"))
    );
}

#[test]
fn a_400_that_stays_a_400_is_tried_only_twice() {
    let body = "{\"error\":\"max_completion_tokens is not supported\"}".to_string();
    let fake = Fake::start(vec![Reply::Status(400, body)]);
    assert_eq!(call(&fake), json!({ "result": "error", "status": 400 }));
    assert_eq!(fake.requests(), 2);
}

#[test]
fn a_400_about_something_else_is_not_retried() {
    let fake = Fake::start(vec![Reply::Status(
        400,
        "{\"error\":\"bad model name\"}".to_string(),
    )]);
    assert_eq!(call(&fake), json!({ "result": "error", "status": 400 }));
    assert_eq!(fake.requests(), 1);
}

#[test]
fn the_deadline_is_shared_by_the_first_request_and_the_retry() {
    let refusal = "{\"error\":\"unknown parameter max_completion_tokens\"}".to_string();
    let fake = Fake::start(vec![
        Reply::Slow(Duration::from_millis(700), 400, refusal),
        Reply::Hang,
    ]);
    let started = Instant::now();
    let line = run("", &stored(&fake.url()), Duration::from_millis(1000));
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap(),
        json!({ "result": "timeout" })
    );
    assert_eq!(fake.requests(), 2);
    // Two full deadlines would take 1.7 s.
    assert!(
        started.elapsed() < Duration::from_millis(1400),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn an_answer_that_stalls_after_the_headers_is_a_timeout() {
    let fake = Fake::start(vec![Reply::Stall]);
    let line = run("", &stored(&fake.url()), Duration::from_millis(600));
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap(),
        json!({ "result": "timeout" })
    );
}

#[test]
fn a_port_with_nothing_behind_it_is_unreachable() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let line = run("", &stored(&format!("http://127.0.0.1:{port}/v1")), LONG);
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap(),
        json!({ "result": "unreachable" })
    );
    assert!(!line.contains(KEY));
}

#[test]
fn a_provider_that_never_answers_ends_at_the_deadline() {
    let fake = Fake::start(vec![Reply::Hang]);
    let started = Instant::now();
    let line = run("", &stored(&fake.url()), Duration::from_millis(600));
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap(),
        json!({ "result": "timeout" })
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_redirect_is_not_followed_and_the_key_stays_here() {
    let elsewhere = Fake::start(vec![completion("pong")]);
    let fake = Fake::start(vec![Reply::Redirect(format!(
        "{}/chat/completions",
        elsewhere.url()
    ))]);
    assert_eq!(call(&fake), json!({ "result": "error", "status": 302 }));
    assert_eq!(fake.requests(), 1);
    thread::sleep(Duration::from_millis(200));
    assert_eq!(elsewhere.requests(), 0);
}

#[test]
fn arguments_win_over_the_store_and_the_default_model_fills_the_gap() {
    let fake = Fake::start(vec![completion("pong")]);
    let stored = Model {
        base_url: "https://elsewhere.invalid/v1".to_string(),
        model: String::new(),
        api_key: Some("stored-key".to_string()),
    };
    let argument = json!({ "base_url": fake.url(), "api_key": KEY }).to_string();
    let line = run(&argument, &stored, LONG);
    assert!(line.contains("works"), "{line}");
    let seen = fake.seen.lock().unwrap();
    assert_eq!(seen[0].body["model"], DEFAULT_MODEL);
    assert_eq!(
        seen[0].authorization.as_deref(),
        Some(&*format!("Bearer {KEY}"))
    );
}

#[test]
fn a_call_that_cannot_be_made_is_refused_before_any_request() {
    let fake = Fake::start(vec![completion("pong")]);
    let none = Model::default();
    let refused = |argument: &str, model: &Model| -> String {
        let line = run(argument, model, LONG);
        assert!(line.contains("\"error\""), "{line}");
        assert!(!line.contains(KEY), "{line}");
        line
    };
    refused("", &none);
    refused("{\"base_url\":\"http://example.com/v1\"}", &none);
    refused("{\"base_url\":\"https://user:pw@example.com/v1\"}", &none);
    refused("{\"base_url\":\"http://localhost.evil.com/v1\"}", &none);
    refused("{\"base_url\":\"http://127.0.0.1@evil.com/v1\"}", &none);
    refused(
        &json!({ "base_url": fake.url(), "api_key": "a b\nc" }).to_string(),
        &none,
    );
    refused(&format!("{{\"api_key\":\"{KEY}\",\"surprise\":1}}"), &none);
    refused(&format!("not json {KEY}"), &none);
    assert_eq!(fake.requests(), 0);
}

#[test]
fn the_socket_command_runs_the_test_over_the_store() {
    let fake = Fake::start(vec![completion("pong")]);
    let dir =
        std::env::temp_dir().join(format!("construct-agent-model-test-{}", std::process::id()));
    let settings = Settings::open(&dir.join("config")).unwrap();
    let socket = dir.join("run").join("agent.sock");
    let server = Server::bind(&socket).unwrap().with_settings(settings);
    thread::spawn(move || server.serve());

    let ask = |line: &str| -> String {
        let mut s = UnixStream::connect(&socket).unwrap();
        s.write_all(format!("{line}\n").as_bytes()).unwrap();
        let mut reply = String::new();
        s.read_to_string(&mut reply).unwrap();
        reply
    };
    let set = json!({ "model": { "base_url": fake.url(), "model": "m", "api_key": KEY } });
    ask(&format!("settings-set {set}"));
    let reply = ask("settings-test");
    assert_eq!(
        serde_json::from_str::<Value>(&reply).unwrap(),
        json!({ "result": "works", "text": "pong" })
    );
    assert!(!reply.contains(KEY));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_stored_key_is_not_sent_to_another_address() {
    let elsewhere = Fake::start(vec![completion("pong")]);
    // The stored address is another one, so the stored key must stay home.
    let stored = stored("http://127.0.0.1:9/v1");
    let argument = json!({ "base_url": elsewhere.url() }).to_string();
    let line = run(&argument, &stored, LONG);
    assert!(line.contains("works"), "{line}");
    assert_eq!(elsewhere.seen.lock().unwrap()[0].authorization, None);
}

#[test]
fn a_key_that_cannot_be_a_header_is_refused_before_any_request() {
    // `settings-set` takes any key without control characters, but a header cannot carry a
    // non-ASCII one: say so, instead of "unreachable".
    let fake = Fake::start(vec![completion("pong")]);
    let argument = json!({ "base_url": fake.url(), "api_key": "sk-ключ" }).to_string();
    let line = run(&argument, &Model::default(), LONG);
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap(),
        json!({ "error": "invalid api_key" })
    );
    assert!(!line.contains("ключ"));
}

#[test]
fn the_socket_command_without_settings_answers_an_error() {
    let dir = std::env::temp_dir().join(format!(
        "construct-agent-model-test-none-{}",
        std::process::id()
    ));
    let socket = dir.join("run").join("agent.sock");
    let server = Server::bind(&socket).unwrap();
    thread::spawn(move || server.serve());
    let mut s = UnixStream::connect(&socket).unwrap();
    s.write_all(b"settings-test\n").unwrap();
    let mut reply = String::new();
    s.read_to_string(&mut reply).unwrap();
    assert!(reply.contains("settings are not available"), "{reply}");
    let _ = std::fs::remove_dir_all(&dir);
}
