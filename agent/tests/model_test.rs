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
use construct_agent::model_test::{Check, DEFAULT_MODEL, checks, run_checks};
use construct_agent::settings::{Model, Settings};
use serde_json::{Value, json};

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

fn completion_body(text: &str) -> String {
    json!({ "choices": [{ "message": { "role": "assistant", "content": text } }] }).to_string()
}

fn completion(text: &str) -> Reply {
    Reply::Status(200, completion_body(text))
}

fn stored(base_url: &str) -> Model {
    Model {
        base_url: base_url.to_string(),
        model: "my-model".to_string(),
        api_key: Some(KEY.to_string()),
    }
}

/// Run the check `answers` alone and give its line without the `checks` list: the tests of
/// this check are about its result.
fn run(argument: &str, stored: &Model, deadline: Duration) -> String {
    let mut last = String::new();
    run_checks(argument, stored, deadline, &[Check::answers()], &mut |l| {
        last = l.to_string();
        true
    });
    match serde_json::from_str::<Value>(&last) {
        Ok(mut line) if line.get("checks").is_some() => {
            line.as_object_mut().unwrap().remove("checks");
            line.to_string()
        }
        _ => last,
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
    assert!(
        messages
            .iter()
            .any(|m| m["role"] == "user" && m["content"] == "ping")
    );
    assert!(
        messages
            .iter()
            .any(|m| m["content"].as_str().unwrap().contains("pong"))
    );
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
    assert_eq!(reply.lines().count(), 1, "{reply}");
    // The fake answers "pong" to everything, so the model never calls the tool.
    assert_eq!(
        serde_json::from_str::<Value>(&reply).unwrap(),
        json!({
            "result": "no tool call",
            "checks": [
                { "name": "answers", "result": "works" },
                { "name": "uses tools", "result": "no tool call" },
            ],
        })
    );
    assert!(!reply.contains(KEY));
    let reply = ask("settings-test {\"progress\":true}");
    let lines: Vec<Value> = reply
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 4, "{reply}");
    assert_eq!(lines[0], json!({ "checks": ["answers", "uses tools"] }));
    assert_eq!(lines[3]["result"], "no tool call");
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

// The whole Test: the checks, in order.

/// The model calls the tool.
fn tool_call(arguments: &str) -> Reply {
    Reply::Status(
        200,
        json!({ "choices": [{ "message": {
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_7",
                "type": "function",
                "function": { "name": "add", "arguments": arguments },
            }],
        } }] })
        .to_string(),
    )
}

/// Run the whole Test, and give every line it answered with.
fn test_lines(fake: &Fake, argument: &str) -> Vec<String> {
    test_lines_within(fake, argument, LONG)
}

fn test_lines_within(fake: &Fake, argument: &str, deadline: Duration) -> Vec<String> {
    let mut lines = Vec::new();
    run_checks(
        argument,
        &stored(&fake.url()),
        deadline,
        &checks(),
        &mut |l| {
            lines.push(l.to_string());
            true
        },
    );
    for line in &lines {
        assert!(!line.contains(KEY), "the key is in the answer: {line}");
    }
    lines
}

fn last(lines: &[String]) -> Value {
    serde_json::from_str(lines.last().unwrap()).unwrap()
}

#[test]
fn a_model_that_answers_and_uses_the_tool_works_in_both_checks() {
    let fake = Fake::start(vec![
        completion("pong"),
        tool_call("{\"a\":2,\"b\":3}"),
        completion("2 + 3 = 5"),
    ]);
    let lines = test_lines(&fake, "");
    assert_eq!(lines.len(), 1);
    assert_eq!(
        last(&lines),
        json!({
            "result": "works",
            "text": "pong",
            "checks": [
                { "name": "answers", "result": "works" },
                { "name": "uses tools", "result": "works" },
            ],
        })
    );

    let seen = fake.seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(seen[0].body.get("tools").is_none());
    // The tool request offers the tool and leaves the model room to think.
    assert_eq!(seen[1].body["tools"][0]["function"]["name"], "add");
    assert_eq!(seen[1].body["max_completion_tokens"], 1024);
    // The second request answers the call with the tool's result.
    let messages = seen[2].body["messages"].as_array().unwrap();
    let tool = messages.last().unwrap();
    assert_eq!(tool["role"], "tool");
    assert_eq!(tool["tool_call_id"], "call_7");
    assert_eq!(tool["content"], "5");
    let call = &messages[messages.len() - 2];
    assert_eq!(call["role"], "assistant");
    assert_eq!(call["tool_calls"][0]["id"], "call_7");
    assert_eq!(seen[2].body["tools"][0]["function"]["name"], "add");
    for captured in seen.iter() {
        assert_eq!(
            captured.authorization.as_deref(),
            Some(&*format!("Bearer {KEY}"))
        );
    }
}

#[test]
fn arguments_in_the_other_order_fit() {
    let fake = Fake::start(vec![
        completion("pong"),
        tool_call("{\"a\":3.0,\"b\":2}"),
        completion("5"),
    ]);
    assert_eq!(last(&test_lines(&fake, ""))["result"], "works");
}

#[test]
fn text_instead_of_a_tool_call_is_no_tool_call_and_the_test_result() {
    let fake = Fake::start(vec![completion("pong"), completion("The sum is 5.")]);
    let lines = test_lines(&fake, "");
    assert_eq!(
        last(&lines),
        json!({
            "result": "no tool call",
            "checks": [
                { "name": "answers", "result": "works" },
                { "name": "uses tools", "result": "no tool call" },
            ],
        })
    );
    assert_eq!(fake.requests(), 2);
}

#[test]
fn a_call_with_arguments_that_do_not_fit_is_a_bad_tool_call() {
    for arguments in [
        "{\"a\":2}",
        "{\"a\":2,\"b\":4}",
        "{\"a\":\"2\",\"b\":\"3\"}",
        "{\"a\":2,\"b\":3,\"c\":1}",
        "not json",
        "[2,3]",
    ] {
        let fake = Fake::start(vec![completion("pong"), tool_call(arguments)]);
        let answer = last(&test_lines(&fake, ""));
        assert_eq!(answer["result"], "bad tool call", "{arguments}");
        assert_eq!(fake.requests(), 2);
    }
    // A call of a tool that was not offered.
    let wrong_name = tool_call("{\"a\":2,\"b\":3}");
    let Reply::Status(_, body) = wrong_name else {
        unreachable!()
    };
    let fake = Fake::start(vec![
        completion("pong"),
        Reply::Status(200, body.replace("\"add\"", "\"subtract\"")),
    ]);
    assert_eq!(last(&test_lines(&fake, ""))["result"], "bad tool call");
}

#[test]
fn a_provider_that_refuses_tools_gives_tools_refused_and_no_body() {
    let echo = format!("{{\"error\":\"this model does not support tools: {KEY}\"}}");
    for status in [400, 404, 422] {
        let fake = Fake::start(vec![
            completion("pong"),
            Reply::Status(status, echo.clone()),
        ]);
        let lines = test_lines(&fake, "");
        assert_eq!(
            last(&lines),
            json!({
                "result": "tools refused",
                "checks": [
                    { "name": "answers", "result": "works" },
                    { "name": "uses tools", "result": "tools refused" },
                ],
            }),
            "status {status}"
        );
        assert!(!lines[0].contains("support"));
    }
}

#[test]
fn a_400_about_max_completion_tokens_is_retried_in_the_tool_check_too() {
    let fake = Fake::start(vec![
        completion("pong"),
        Reply::Status(
            400,
            "{\"error\":\"unknown parameter max_completion_tokens\"}".to_string(),
        ),
        tool_call("{\"a\":2,\"b\":3}"),
        completion("5"),
    ]);
    assert_eq!(last(&test_lines(&fake, ""))["result"], "works");
    let seen = fake.seen.lock().unwrap();
    assert_eq!(seen.len(), 4);
    assert_eq!(seen[2].body["max_tokens"], 1024);
    assert!(seen[2].body.get("max_completion_tokens").is_none());
}

#[test]
fn an_answer_that_does_not_use_the_result_is_a_wrong_answer() {
    for second in [completion("I cannot tell"), completion(""), tool_call("{}")] {
        let fake = Fake::start(vec![
            completion("pong"),
            tool_call("{\"a\":2,\"b\":3}"),
            second,
        ]);
        assert_eq!(last(&test_lines(&fake, ""))["result"], "wrong answer");
    }
}

#[test]
fn other_failures_of_the_tool_check_keep_their_reasons() {
    for (status, want) in [
        (401, json!({ "result": "key refused" })),
        (500, json!({ "result": "error", "status": 500 })),
    ] {
        let fake = Fake::start(vec![
            completion("pong"),
            Reply::Status(status, "{}".to_string()),
        ]);
        let answer = last(&test_lines(&fake, ""));
        assert_eq!(answer["result"], want["result"], "status {status}");
        assert_eq!(answer.get("status"), want.get("status"));
    }
    let fake = Fake::start(vec![
        completion("pong"),
        Reply::Status(200, "<html></html>".to_string()),
    ]);
    let answer = last(&test_lines(&fake, ""));
    assert_eq!(answer["result"], "error");
    assert_eq!(answer["status"], 200);
}

#[test]
fn a_failed_second_request_of_the_tool_check_is_not_hidden() {
    let fake = Fake::start(vec![
        completion("pong"),
        tool_call("{\"a\":2,\"b\":3}"),
        Reply::Status(500, "{}".to_string()),
    ]);
    let answer = last(&test_lines(&fake, ""));
    assert_eq!(answer["result"], "error");
    assert_eq!(answer["status"], 500);
}

#[test]
fn a_bad_key_fails_the_first_check_and_skips_the_second() {
    let fake = Fake::start(vec![Reply::Status(401, "{}".to_string())]);
    let lines = test_lines(&fake, "");
    assert_eq!(
        last(&lines),
        json!({
            "result": "key refused",
            "checks": [
                { "name": "answers", "result": "key refused" },
                { "name": "uses tools", "result": "skipped" },
            ],
        })
    );
    assert_eq!(fake.requests(), 1);
}

#[test]
fn an_error_status_is_kept_in_the_entry_of_its_check() {
    let fake = Fake::start(vec![Reply::Status(500, "{}".to_string())]);
    let answer = last(&test_lines(&fake, ""));
    assert_eq!(answer["result"], "error");
    assert_eq!(answer["status"], 500);
    assert_eq!(
        answer["checks"][0],
        json!({ "name": "answers", "result": "error", "status": 500 })
    );
}

#[test]
fn with_progress_the_lines_come_in_order_and_the_last_is_the_one_line_reply() {
    let script = || {
        vec![
            completion("pong"),
            tool_call("{\"a\":2,\"b\":3}"),
            completion("5"),
        ]
    };
    let plain = test_lines(&Fake::start(script()), "");
    assert_eq!(plain.len(), 1);

    let fake = Fake::start(script());
    let lines = test_lines(&fake, "{\"progress\":true}");
    assert_eq!(lines.len(), 4, "{lines:?}");
    let parsed: Vec<Value> = lines
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(parsed[0], json!({ "checks": ["answers", "uses tools"] }));
    assert_eq!(
        parsed[1],
        json!({ "check": { "name": "answers", "result": "works" } })
    );
    assert_eq!(
        parsed[2],
        json!({ "check": { "name": "uses tools", "result": "works" } })
    );
    assert_eq!(lines[3], plain[0]);
}

#[test]
fn a_checks_line_arrives_before_the_next_check_starts() {
    let fake = Fake::start(vec![
        completion("pong"),
        tool_call("{\"a\":2,\"b\":3}"),
        completion("5"),
    ]);
    // What the fake had seen when each line arrived.
    let mut seen_at = Vec::new();
    run_checks(
        "{\"progress\":true}",
        &stored(&fake.url()),
        LONG,
        &checks(),
        &mut |l| {
            seen_at.push((l.to_string(), fake.requests()));
            true
        },
    );
    assert_eq!(seen_at.len(), 4);
    assert_eq!(seen_at[0].1, 0, "the names come first");
    assert!(seen_at[1].0.contains("\"answers\""));
    assert_eq!(
        seen_at[1].1, 1,
        "the first check ended before the second began"
    );
    assert_eq!(seen_at[2].1, 3);
}

#[test]
fn with_progress_a_skipped_check_has_its_line_and_a_bad_request_one_line() {
    let fake = Fake::start(vec![Reply::Status(401, "{}".to_string())]);
    let lines = test_lines(&fake, "{\"progress\":true}");
    assert_eq!(lines.len(), 4);
    assert_eq!(
        serde_json::from_str::<Value>(&lines[2]).unwrap(),
        json!({ "check": { "name": "uses tools", "result": "skipped" } })
    );

    let lines = test_lines(&fake, "{\"progress\":true,\"surprise\":1}");
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("\"error\""));
    assert_eq!(fake.requests(), 1);
}

#[test]
fn the_test_stops_when_nobody_reads_the_lines() {
    let fake = Fake::start(vec![completion("pong")]);
    let mut count = 0;
    run_checks(
        "{\"progress\":true}",
        &stored(&fake.url()),
        LONG,
        &checks(),
        &mut |_| {
            count += 1;
            count < 2
        },
    );
    assert_eq!(count, 2);
    assert_eq!(fake.requests(), 1);
}

#[test]
fn each_check_has_its_own_deadline() {
    let fake = Fake::start(vec![
        Reply::Slow(Duration::from_millis(700), 200, completion_body("pong")),
        Reply::Slow(
            Duration::from_millis(700),
            200,
            completion_body("not a call"),
        ),
    ]);
    // Together the two checks take 1.4 s, over the deadline of one.
    let lines = test_lines_within(&fake, "", Duration::from_millis(1000));
    assert_eq!(last(&lines)["result"], "no tool call");
}

#[test]
fn the_tool_check_ends_at_its_deadline() {
    let fake = Fake::start(vec![completion("pong"), Reply::Hang]);
    let started = Instant::now();
    let lines = test_lines_within(&fake, "", Duration::from_millis(600));
    assert_eq!(last(&lines)["result"], "timeout");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn the_two_requests_of_the_tool_check_share_its_deadline() {
    let call = || {
        let Reply::Status(_, body) = tool_call("{\"a\":2,\"b\":3}") else {
            unreachable!()
        };
        body
    };
    let fake = Fake::start(vec![
        completion("pong"),
        Reply::Slow(Duration::from_millis(700), 200, call()),
        Reply::Hang,
    ]);
    let started = Instant::now();
    let lines = test_lines_within(&fake, "", Duration::from_millis(1000));
    assert_eq!(last(&lines)["result"], "timeout");
    // Two full deadlines would take 1.7 s for the check alone.
    assert!(
        started.elapsed() < Duration::from_millis(1600),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_refusal_after_the_tool_message_is_tools_refused_too() {
    for status in [400, 404, 422] {
        let fake = Fake::start(vec![
            completion("pong"),
            tool_call("{\"a\":2,\"b\":3}"),
            Reply::Status(status, "{}".to_string()),
        ]);
        assert_eq!(
            last(&test_lines(&fake, ""))["result"],
            "tools refused",
            "status {status}"
        );
    }
}

#[test]
fn only_the_whole_number_5_is_the_sum() {
    for (text, want) in [
        ("The sum is 15", "wrong answer"),
        ("2 + 3 = 25", "wrong answer"),
        ("five", "wrong answer"),
        ("5", "works"),
        ("The sum of 2 and 3 is 5.", "works"),
        // A long thought first: the 5 is past the part of the text that is passed on.
        (&format!("{} so it is 5", "thinking ".repeat(50)), "works"),
    ] {
        let fake = Fake::start(vec![
            completion("pong"),
            tool_call("{\"a\":2,\"b\":3}"),
            completion(text),
        ]);
        assert_eq!(last(&test_lines(&fake, ""))["result"], want, "{text}");
    }
}

#[test]
fn a_tool_call_without_an_id_is_a_bad_tool_call() {
    let Reply::Status(_, body) = tool_call("{\"a\":2,\"b\":3}") else {
        unreachable!()
    };
    let without_id = body.replace("\"id\":\"call_7\",", "");
    assert_ne!(without_id, body);
    let fake = Fake::start(vec![completion("pong"), Reply::Status(200, without_id)]);
    assert_eq!(last(&test_lines(&fake, ""))["result"], "bad tool call");
}
