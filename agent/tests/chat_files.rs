//! The file tools through the whole chat loop: a real chat with the four tools over a real
//! workspace folder, answering a scripted fake provider.
#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use construct_agent::chat::Chat;
use construct_agent::chat::files::file_tools;
use construct_agent::chat::tool::ChatTools;
use construct_agent::ipc::Server;
use construct_agent::settings::{Settings, Update, config_dir_from};
use construct_agent::workspace::{DIR_NAME, Workspace};
use serde_json::{Value, json};

mod common;

use common::stand::{chat_send, deadlines};
use common::{Agent, done, head, raw_chunk, read_request};

static NEXT: AtomicU32 = AtomicU32::new(0);

/// One model turn: the calls it asks for as `(name, arguments)`, or a text when there are none.
enum Turn {
    Calls(Vec<(&'static str, Value)>),
    Text(&'static str),
}

/// A provider that answers request `n` with `turns[n]` and notes every request body.
fn provider(turns: Vec<Turn>) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let noted = Arc::clone(&bodies);
    let turns = Arc::new(turns);
    thread::spawn(move || {
        let count = Arc::new(AtomicU32::new(0));
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let (body, _) = read_request(&mut stream);
            noted.lock().unwrap().push(body);
            let n = count.fetch_add(1, Ordering::SeqCst) as usize;
            let turns = Arc::clone(&turns);
            thread::spawn(move || {
                head(&mut stream);
                let reason = match turns.get(n) {
                    Some(Turn::Calls(calls)) => {
                        for (at, (name, arguments)) in calls.iter().enumerate() {
                            let call = json!({ "index": at, "id": format!("call_{n}_{at}"),
                                "type": "function",
                                "function": { "name": name, "arguments": arguments.to_string() } });
                            raw_chunk(
                                &mut stream,
                                &json!({ "choices": [{ "delta": { "tool_calls": [call] } }] }),
                            );
                        }
                        "tool_calls"
                    }
                    Some(Turn::Text(text)) => {
                        raw_chunk(
                            &mut stream,
                            &json!({ "choices": [{ "delta": { "content": text } }] }),
                        );
                        "stop"
                    }
                    None => "stop",
                };
                raw_chunk(
                    &mut stream,
                    &json!({ "choices": [{ "delta": {}, "finish_reason": reason }] }),
                );
                done(&mut stream);
            });
        }
    });
    (addr, bodies)
}

struct Stand {
    base: PathBuf,
    workspace: PathBuf,
    socket: PathBuf,
    bodies: Arc<Mutex<Vec<String>>>,
    _rt: tokio::runtime::Runtime,
}

impl Stand {
    fn new(turns: Vec<Turn>) -> Self {
        let base = std::env::temp_dir().join(format!(
            "cb-chat-files-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let (data, socket) = (base.join("data"), base.join("run").join("agent.sock"));
        let (addr, bodies) = provider(turns);
        let settings = Settings::open(&base.join("config")).unwrap();
        let model = json!({ "model": {
            "base_url": format!("http://127.0.0.1:{}/v1", addr.port()), "model": "m", "api_key": "k" } });
        settings
            .apply(Update::parse(&model.to_string()).unwrap())
            .unwrap();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let workspace = Arc::new(Workspace::open(&data).unwrap());
        let chat = Chat::new(settings.clone(), &data, rt.handle().clone(), deadlines())
            .unwrap()
            .with_tools(ChatTools::new(file_tools(&workspace)).unwrap());
        let server = Server::bind(&socket)
            .unwrap()
            .with_settings(settings)
            .with_chat(chat);
        thread::spawn(move || server.serve());
        Self {
            workspace: data.join(DIR_NAME),
            base,
            socket,
            bodies,
            _rt: rt,
        }
    }

    /// Sends one message and returns the events of the answer.
    fn say(&self, message: &str) -> Vec<Value> {
        chat_send(&self.socket, "", message).1
    }

    /// The results the model got in its last request, in order.
    fn results(&self) -> Vec<String> {
        let bodies = self.bodies.lock().unwrap();
        let last: Value = serde_json::from_str(bodies.last().unwrap()).unwrap();
        last["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["role"] == "tool")
            .map(|m| m["content"].as_str().unwrap().to_string())
            .collect()
    }

    fn lines(events: &[Value]) -> Vec<String> {
        events
            .iter()
            .filter(|e| e["event"] == "tool")
            .map(|e| e["line"].as_str().unwrap().to_string())
            .collect()
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

#[test]
fn a_glob_read_write_and_edit_round_trip_changes_the_file_and_the_model_goes_on() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![(
            "Write",
            json!({ "file_path": "notes/todo.txt", "content": "buy milk\nwrite code\n" }),
        )]),
        Turn::Calls(vec![("Glob", json!({ "pattern": "**/*.txt" }))]),
        Turn::Calls(vec![("Read", json!({ "file_path": "notes/todo.txt" }))]),
        Turn::Calls(vec![(
            "Edit",
            json!({ "file_path": "notes/todo.txt", "old_string": "milk", "new_string": "bread" }),
        )]),
        Turn::Calls(vec![(
            "Read",
            json!({ "file_path": "notes/todo.txt", "offset": 2, "limit": 1 }),
        )]),
        Turn::Text("Finished."),
    ]);
    let events = stand.say("tidy my notes");
    assert_eq!(events.last().unwrap(), &json!({ "event": "done" }));
    assert_eq!(
        stand.results(),
        [
            "Wrote 20 bytes to notes/todo.txt",
            "notes/todo.txt",
            "     1\tbuy milk\n     2\twrite code",
            "Changed notes/todo.txt",
            "     2\twrite code",
        ]
    );
    let file = std::fs::read_to_string(stand.workspace.join("notes/todo.txt")).unwrap();
    assert_eq!(file, "buy bread\nwrite code\n");
    assert_eq!(stand.bodies.lock().unwrap().len(), 6);
}

#[test]
fn each_error_goes_back_as_a_result_and_the_model_goes_on() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![
            (
                "Write",
                json!({ "file_path": "a.txt", "content": "x\nx\n" }),
            ),
            ("Read", json!({ "file_path": "../outside.txt" })),
            ("Read", json!({ "file_path": "/etc/hostname" })),
            ("Read", json!({ "file_path": "missing.txt" })),
            (
                "Edit",
                json!({ "file_path": "a.txt", "old_string": "x", "new_string": "y" }),
            ),
            (
                "Edit",
                json!({ "file_path": "a.txt", "old_string": "z", "new_string": "y" }),
            ),
            ("Read", json!({ "path": "a.txt" })),
            ("Glob", json!({ "pattern": "[" })),
        ]),
        Turn::Text("I could not do some of that."),
    ]);
    let events = stand.say("try everything");
    assert_eq!(
        events.last().unwrap(),
        &json!({ "event": "done" }),
        "{events:?}"
    );
    assert_eq!(
        stand.results(),
        [
            "Wrote 4 bytes to a.txt",
            "Error: the path is not inside the workspace",
            "Error: the path is not inside the workspace",
            "Error: no such file or folder",
            "Error: the text to replace occurs more than once",
            "Error: the text to replace was not found",
            "Error: the arguments are not valid for this tool",
            "Error: the glob pattern is not valid",
        ]
    );
    // Nothing was changed by the refused calls.
    let file = std::fs::read_to_string(stand.workspace.join("a.txt")).unwrap();
    assert_eq!(file, "x\nx\n");
    assert!(!stand.base.join("data/outside.txt").exists());
}

#[test]
fn replace_all_changes_every_place() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![
            (
                "Write",
                json!({ "file_path": "a.txt", "content": "x\nx\n" }),
            ),
            (
                "Edit",
                json!({ "file_path": "a.txt", "old_string": "x", "new_string": "y",
                    "replace_all": true }),
            ),
        ]),
        Turn::Text("Done."),
    ]);
    stand.say("replace");
    assert_eq!(stand.results()[1], "Changed a.txt");
    let file = std::fs::read_to_string(stand.workspace.join("a.txt")).unwrap();
    assert_eq!(file, "y\ny\n");
}

#[test]
fn the_tools_array_holds_the_four_in_order_and_is_the_same_in_every_request() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![("Glob", json!({ "pattern": "*" }))]),
        Turn::Text("None."),
    ]);
    stand.say("first");
    stand.say("second");
    let bodies = stand.bodies.lock().unwrap().clone();
    assert!(bodies.len() >= 3, "{}", bodies.len());
    let tools = |body: &str| serde_json::from_str::<Value>(body).unwrap()["tools"].to_string();
    let first = tools(&bodies[0]);
    let names: Vec<String> = serde_json::from_str::<Value>(&first)
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["Read", "Write", "Edit", "Glob"]);
    for body in &bodies {
        assert_eq!(tools(body), first);
    }
    // The argument names are those of Claude Code.
    let listed: Value = serde_json::from_str(&first).unwrap();
    let properties = |at: usize| -> Vec<String> {
        listed[at]["function"]["parameters"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    };
    let sorted = |mut names: Vec<String>| {
        names.sort();
        names
    };
    assert_eq!(sorted(properties(0)), ["file_path", "limit", "offset"]);
    assert_eq!(sorted(properties(1)), ["content", "file_path"]);
    assert_eq!(
        sorted(properties(2)),
        ["file_path", "new_string", "old_string", "replace_all"]
    );
    assert_eq!(sorted(properties(3)), ["path", "pattern"]);
}

#[test]
fn a_long_read_is_cut_with_a_note_on_how_to_read_the_rest() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![("Read", json!({ "file_path": "big.txt" }))]),
        Turn::Text("Read it."),
    ]);
    std::fs::create_dir_all(&stand.workspace).unwrap();
    let text = (1..=20_000).fold(String::new(), |mut text, n| {
        let _ = writeln!(text, "line number {n}");
        text
    });
    std::fs::write(stand.workspace.join("big.txt"), &text).unwrap();
    stand.say("read big.txt");
    let result = stand.results().remove(0);
    assert!(result.len() <= 64 * 1024, "{}", result.len());
    let (shown, note) = result.rsplit_once('\n').unwrap();
    let last_shown = shown.lines().count();
    assert!(
        note.contains(&format!("offset {}", last_shown + 1)),
        "{note}"
    );
    assert!(shown.ends_with(&format!("{last_shown:>6}\tline number {last_shown}")));
}

#[test]
fn the_status_lines_name_the_file_or_the_pattern_and_never_the_content() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![
            (
                "Write",
                json!({ "file_path": "w.txt", "content": "TOP-SECRET-CONTENT" }),
            ),
            ("Read", json!({ "file_path": "w.txt" })),
            (
                "Edit",
                json!({ "file_path": "w.txt", "old_string": "TOP", "new_string": "BOTTOM" }),
            ),
            ("Glob", json!({ "pattern": "*.txt" })),
        ]),
        Turn::Text("Done."),
    ]);
    let events = stand.say("go");
    let lines = Stand::lines(&events);
    assert_eq!(
        lines,
        [
            "Wrote w.txt",
            "Read w.txt",
            "Changed w.txt",
            "Searched *.txt"
        ]
    );
    let all = serde_json::to_string(&events).unwrap();
    assert!(!all.contains("SECRET") && !all.contains("BOTTOM"), "{all}");
    assert!(!all.contains(stand.base.to_str().unwrap()), "{all}");
}

/// A provider for the program: a `Write` call, then a text.
fn program_provider(content: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for (n, stream) in listener.incoming().enumerate() {
            let mut stream = stream.unwrap();
            read_request(&mut stream);
            head(&mut stream);
            let (delta, reason) = if n == 0 {
                let arguments = json!({ "file_path": "diary.txt", "content": content });
                let call = json!({ "index": 0, "id": "call_1", "type": "function",
                    "function": { "name": "Write", "arguments": arguments.to_string() } });
                (json!({ "tool_calls": [call] }), "tool_calls")
            } else {
                (json!({ "content": "Written." }), "stop")
            };
            raw_chunk(&mut stream, &json!({ "choices": [{ "delta": delta }] }));
            raw_chunk(
                &mut stream,
                &json!({ "choices": [{ "delta": {}, "finish_reason": reason }] }),
            );
            done(&mut stream);
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

#[test]
fn the_program_gives_the_chat_the_tools_and_logs_no_content_and_no_path() {
    const CONTENT: &str = "dear diary, the red key is under the mat";
    let dir = std::env::temp_dir().join(format!("agent-chat-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let config = config_dir_from(
        Some(dir.join("config")),
        Some(dir.join("home")),
        cfg!(target_os = "macos"),
    )
    .unwrap();
    let settings = Settings::open(&config).unwrap();
    let update = json!({ "model": { "base_url": program_provider(CONTENT), "model": "m",
        "api_key": "k" } });
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
    let answer = send(&socket, "write my diary");
    assert!(answer.contains("Wrote diary.txt"), "{answer}");
    assert!(answer.contains("\"done\""), "{answer}");
    let written = dir.join("data/construct/workspace/diary.txt");
    assert_eq!(std::fs::read_to_string(&written).unwrap(), CONTENT);

    let logged = std::fs::read_to_string(&log).unwrap();
    assert!(logged.contains("chat: answer ended"), "{logged}");
    // The log names the socket at start-up; it never names the workspace folder.
    let folder = dir.join("data/construct/workspace");
    for text in [&logged, &answer] {
        assert!(!text.contains("red key"), "{text}");
        assert!(!text.contains(folder.to_str().unwrap()), "{text}");
    }
    assert!(!answer.contains(dir.to_str().unwrap()), "{answer}");
    let _ = std::fs::remove_dir_all(&dir);
}
