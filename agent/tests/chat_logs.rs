//! The agent as a program, chatting with a provider that quotes the key and the message in its
//! refusal, and with an event that is no JSON: neither the log nor the answers may hold them,
//! and the conversation records hold the message (as they must) but not the key.
#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

mod common;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use common::{Agent, chunk, done, head, raw_chunk, read_request, status};
use construct_agent::settings::{Settings, Update, config_dir_from};
use serde_json::json;

const KEY: &str = "sk-log-test-9c1d";
const MESSAGE: &str = "the blue door opens at noon";

/// Answers the requests in turn: a 401 quoting the key and the message, a 400 with
/// `context_length_exceeded` that quotes them too, a stream with an event that is no JSON and
/// quotes the message, and then clean streams.
fn provider() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for (n, stream) in listener.incoming().enumerate() {
            let mut stream = stream.unwrap();
            read_request(&mut stream);
            let quote = format!("bad key {KEY} for {MESSAGE}");
            match n {
                0 => status(&mut stream, 401, &json!({ "error": quote }).to_string()),
                1 => status(
                    &mut stream,
                    400,
                    &json!({ "error": { "code": "context_length_exceeded", "message": quote } })
                        .to_string(),
                ),
                2 => {
                    head(&mut stream);
                    write!(stream, "data: not json, {quote}\n\n").unwrap();
                    raw_chunk(
                        &mut stream,
                        &json!({ "choices": [{ "delta": {}, "usage": quote }] }),
                    );
                    chunk(&mut stream, "fine");
                    done(&mut stream);
                }
                // A turn with a tool call whose name and arguments quote the key and the message.
                4 => {
                    head(&mut stream);
                    let call = json!({ "index": 0, "id": "call_1", "function": {
                        "name": format!("tool_{KEY}"), "arguments": json!({ "q": quote }).to_string()
                    } });
                    raw_chunk(
                        &mut stream,
                        &json!({ "choices": [{ "delta": { "tool_calls": [call] } }] }),
                    );
                    raw_chunk(
                        &mut stream,
                        &json!({ "choices": [{ "delta": {}, "finish_reason": "tool_calls" }] }),
                    );
                    done(&mut stream);
                }
                _ => {
                    head(&mut stream);
                    chunk(&mut stream, "fine");
                    done(&mut stream);
                }
            }
        }
    });
    format!("http://127.0.0.1:{port}/v1")
}

fn send(socket: &Path, message: &str) -> String {
    let mut s = UnixStream::connect(socket).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    writeln!(s, "chat-send {}", json!({ "message": message })).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

fn files_under(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, found);
        } else {
            found.push(path);
        }
    }
}

#[test]
fn neither_the_log_nor_the_records_hold_the_key_and_the_log_no_message() {
    let dir = std::env::temp_dir().join(format!("agent-chat-logs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let config = config_dir_from(
        Some(dir.join("config")),
        Some(dir.join("home")),
        cfg!(target_os = "macos"),
    )
    .unwrap();
    let settings = Settings::open(&config).unwrap();
    let update = json!({ "model": { "base_url": provider(), "model": "m", "api_key": KEY } });
    settings
        .apply(Update::parse(&update.to_string()).unwrap())
        .unwrap();

    let log = dir.join("agent.log");
    let _agent = Agent(
        Command::new(env!("CARGO_BIN_EXE_construct-agent"))
            .arg("run")
            .env("HOME", dir.join("home"))
            .env("XDG_RUNTIME_DIR", dir.join("run"))
            .env("XDG_DATA_HOME", dir.join("data"))
            .env("XDG_CONFIG_HOME", dir.join("config"))
            .env("TMPDIR", dir.join("run"))
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&log).unwrap())
            .spawn()
            .unwrap(),
    );
    let socket = dir.join("run/construct/agent.sock");
    for _ in 0..400 {
        if UnixStream::connect(&socket).is_ok() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    // Each answer ends with a line in the log, written before the client gets the last line.
    for (n, expected) in ["key refused", "too long", "\"done\"", "\"done\""]
        .into_iter()
        .enumerate()
    {
        let answer = send(&socket, MESSAGE);
        assert!(answer.contains(expected), "{answer}");
        assert!(
            !answer.contains(KEY) && !answer.contains("blue door"),
            "{answer}"
        );
        let logged = std::fs::read_to_string(&log).unwrap();
        assert_eq!(
            logged.matches("chat: answer ended").count(),
            n + 1,
            "{logged}"
        );
        assert!(
            !logged.contains(KEY) && !logged.contains("blue door"),
            "{logged}"
        );
    }

    // The conversation records hold the message, never the key.
    let mut files = Vec::new();
    files_under(&dir.join("data/construct/records"), &mut files);
    assert_eq!(files.len(), 4);
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.contains(MESSAGE) && !text.contains(KEY),
            "{}",
            file.display()
        );
    }

    // An answer with a tool call that quotes the key and the message: the call is stored as the
    // model wrote it, but the log still has neither.
    let answer = send(&socket, MESSAGE);
    assert!(answer.contains("\"done\""), "{answer}");
    assert!(
        !answer.contains(KEY) && !answer.contains("blue door"),
        "{answer}"
    );
    let logged = std::fs::read_to_string(&log).unwrap();
    assert_eq!(logged.matches("chat: answer ended").count(), 5, "{logged}");
    assert!(
        !logged.contains(KEY) && !logged.contains("blue door"),
        "{logged}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
