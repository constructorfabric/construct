//! The `run_subagent` tool through the whole chat loop: a real chat with the file tools and the
//! sub-agent tool over a real workspace folder, answering a scripted fake provider.
#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::io::{BufRead, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use construct_agent::chat::files::file_tools;
use construct_agent::chat::tool::ChatTools;
use construct_agent::chat::{Chat, Usage, subagent};
use construct_agent::ipc::Server;
use construct_agent::settings::{Settings, Update};
use construct_agent::workspace::{DIR_NAME, Workspace};
use serde_json::{Value, json};

mod common;

use common::scripted::{Turn, provider};
use common::stand::{connect, deadlines, wait_for};

static NEXT: AtomicU32 = AtomicU32::new(0);

struct Stand {
    base: PathBuf,
    workspace: PathBuf,
    socket: PathBuf,
    bodies: Arc<Mutex<Vec<String>>>,
    usage: Arc<Mutex<Vec<Usage>>>,
    _rt: tokio::runtime::Runtime,
}

impl Stand {
    fn new(turns: Vec<Turn>) -> Self {
        let base = std::env::temp_dir().join(format!(
            "cb-chat-subagent-{}-{}",
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
        let mut tools = file_tools(&workspace);
        tools.push(subagent::tool());
        let usage = Arc::new(Mutex::new(Vec::new()));
        let reported = Arc::clone(&usage);
        let chat = Chat::new(settings.clone(), &data, rt.handle().clone(), deadlines())
            .unwrap()
            .with_tools(ChatTools::new(tools).unwrap())
            .with_usage(move |u| reported.lock().unwrap().push(u));
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
            usage,
            _rt: rt,
        }
    }

    /// Sends one message and returns the events of the answer, however it ends.
    fn say(&self, message: &str) -> Vec<Value> {
        connect(
            &self.socket,
            &format!("chat-send {}", json!({ "message": message })),
        )
    }

    fn bodies(&self) -> Vec<Value> {
        self.bodies
            .lock()
            .unwrap()
            .iter()
            .map(|body| serde_json::from_str(body).unwrap())
            .collect()
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn messages(body: &Value) -> Vec<Value> {
    body["messages"].as_array().unwrap().clone()
}

fn tools_text(body: &Value) -> String {
    body["tools"].to_string()
}

fn names(body: &Value) -> Vec<String> {
    body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap().to_string())
        .collect()
}

/// The bytes of `Turn::Calls` as the loop counts them: names and arguments.
fn call_bytes(calls: &[(&str, Value)]) -> u64 {
    calls
        .iter()
        .map(|(name, arguments)| name.len() + arguments.to_string().len())
        .sum::<usize>() as u64
}

#[test]
fn a_sub_agent_works_in_its_own_conversation_and_its_last_text_is_the_result() {
    let task = json!({ "task": "find the one note and tell me what it says", "files": ["a.txt"] });
    let read = vec![("Read", json!({ "file_path": "a.txt" }))];
    let stand = Stand::new(vec![
        Turn::Calls(vec![(subagent::NAME, task.clone())]),
        Turn::Calls(read.clone()),
        Turn::Text("The note says: blue door."),
        Turn::Text("Your note says the blue door."),
    ]);
    std::fs::write(stand.workspace.join("a.txt"), "blue door\n").unwrap();

    let events = stand.say("what is in my notes?");
    assert_eq!(events.last().unwrap(), &json!({ "event": "done" }));
    // The window shows the parent's call with the first words of the task, then the sub-agent's
    // own call by its line, as the parent's calls show.
    let lines: Vec<&Value> = events.iter().filter(|e| e["event"] == "tool").collect();
    assert_eq!(
        lines,
        [
            &json!({ "event": "tool", "line": "Asked a sub-agent: find the one note and tell me what…" }),
            &json!({ "event": "tool", "line": "Read a.txt" })
        ]
    );
    // The text of the sub-agent goes to the model only: the window gets the parent's text alone.
    let text: Vec<&Value> = events.iter().filter(|e| e["event"] == "delta").collect();
    assert_eq!(
        text,
        [&json!({ "event": "delta", "text": "Your note says the blue door." })]
    );

    let bodies = stand.bodies();
    assert_eq!(bodies.len(), 4);
    // The sub-agent starts from its own instruction and the task, and holds nothing of the parent.
    let first = messages(&bodies[1]);
    assert_eq!(first.len(), 2);
    assert_eq!(first[0]["role"], "system");
    assert_ne!(first[0]["content"], messages(&bodies[0])[0]["content"]);
    assert_eq!(messages(&bodies[2])[0], first[0]);
    assert_eq!(first[1]["role"], "user");
    let started = first[1]["content"].as_str().unwrap();
    assert!(started.starts_with("find the one note"), "{started}");
    assert!(started.ends_with("Files to start with: a.txt"), "{started}");
    // Its second request has its own call and the file it read.
    let second = messages(&bodies[2]);
    assert_eq!(second.len(), 4);
    assert_eq!(second[3]["role"], "tool");
    assert!(
        second[3]["content"].as_str().unwrap().contains("blue door"),
        "{second:?}"
    );
    // The parent's next request holds the result and none of the sub-agent's messages.
    let next = messages(&bodies[3]);
    assert_eq!(next.len(), 4, "{next:?}");
    assert_eq!(next[3]["role"], "tool");
    assert_eq!(next[3]["content"], "The note says: blue door.");
    let whole = Value::Array(next.clone()).to_string();
    assert!(!whole.contains("Files to start with"), "{whole}");
    assert!(!whole.contains("1\\tblue door"), "{whole}");
    assert!(
        !next
            .iter()
            .any(|m| m["tool_calls"].to_string().contains("Read"))
    );

    // The conversation the user keeps has the call and the result, not the sub-agent's turns.
    let conversations = connect(&stand.socket, "chat-list");
    let id = conversations[0]["conversations"][0]["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    let kept = connect(&stand.socket, &format!("chat-get {id}"));
    let kinds: Vec<&str> = kept.iter().map(|r| r["kind"].as_str().unwrap()).collect();
    // The user's turn, the call with its line (one only: the sub-agent's line is not kept), the
    // result, and the answer.
    assert_eq!(
        kinds,
        ["turn", "toolcall", "turn", "toolresult", "turn"],
        "{kept:?}"
    );
    assert_eq!(kept[1]["name"], subagent::NAME);
    assert_eq!(kept[3]["text"], "The note says: blue door.");
    assert_eq!(kept[4]["text"], "Your note says the blue door.");

    // The sizes: every request body sent, and the text and calls received, the sub-agent's too.
    let sent: u64 = stand
        .bodies
        .lock()
        .unwrap()
        .iter()
        .map(|b| b.len() as u64)
        .sum();
    let received = call_bytes(&[(subagent::NAME, task)])
        + call_bytes(&read)
        + "The note says: blue door.".len() as u64
        + "Your note says the blue door.".len() as u64;
    assert_eq!(*stand.usage.lock().unwrap(), [Usage { sent, received }]);
}

#[test]
fn the_tool_is_listed_once_after_the_others_and_the_list_never_changes() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![(subagent::NAME, json!({ "task": "one" }))]),
        Turn::Text("done one"),
        Turn::Calls(vec![(subagent::NAME, json!({ "task": "two" }))]),
        Turn::Text("done two"),
        Turn::Text("all done"),
    ]);
    stand.say("go");
    let bodies = stand.bodies();
    assert_eq!(bodies.len(), 5);
    let parent = [0, 2, 4];
    for &at in &parent {
        assert_eq!(
            names(&bodies[at]),
            ["Read", "Write", "Edit", "Glob", "run_subagent"]
        );
        assert_eq!(tools_text(&bodies[at]), tools_text(&bodies[0]));
    }
    // A sub-agent has the file tools and not this one, the same in every request.
    for at in [1, 3] {
        assert_eq!(names(&bodies[at]), ["Read", "Write", "Edit", "Glob"]);
        assert_eq!(tools_text(&bodies[at]), tools_text(&bodies[1]));
    }
    // Two sub-agents in a row do not see each other, and start from the same instruction.
    assert_eq!(messages(&bodies[3]).len(), 2);
    assert_eq!(messages(&bodies[3])[0], messages(&bodies[1])[0]);
}

#[test]
fn a_sub_agent_that_asks_for_another_gets_a_plain_error_and_goes_on() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![(subagent::NAME, json!({ "task": "outer" }))]),
        Turn::Calls(vec![(subagent::NAME, json!({ "task": "inner" }))]),
        Turn::Text("I could not ask for help."),
        Turn::Text("finished"),
    ]);
    let events = stand.say("go");
    assert_eq!(events.last().unwrap(), &json!({ "event": "done" }));
    let bodies = stand.bodies();
    // No request of the inner task: only the parent's, the sub-agent's two and the parent's.
    assert_eq!(bodies.len(), 4);
    let reply = messages(&bodies[2]);
    assert_eq!(reply.last().unwrap()["role"], "tool");
    assert_eq!(
        reply.last().unwrap()["content"],
        "Error: a sub-agent cannot start another sub-agent"
    );
    assert_eq!(
        messages(&bodies[3]).last().unwrap()["content"],
        "I could not ask for help."
    );
}

#[test]
fn stop_ends_the_sub_agent_and_the_parent_together() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![(subagent::NAME, json!({ "task": "slow work" }))]),
        Turn::Silence,
        Turn::Text("never asked"),
    ]);
    let socket = stand.socket.clone();
    let sender = thread::spawn(move || {
        connect(
            &socket,
            &format!("chat-send {}", json!({ "message": "start" })),
        )
    });
    // The sub-agent's request is the second one, and the provider does not answer it.
    wait_for("the sub-agent's request", || {
        stand.bodies.lock().unwrap().len() == 2
    });
    let id = connect(&stand.socket, "chat-list")[0]["conversations"][0]["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        connect(&stand.socket, &format!("chat-stop {id}")),
        [json!({ "stopped": true })]
    );
    let events = sender.join().unwrap();
    assert_eq!(
        events.last().unwrap(),
        &json!({ "event": "done", "stopped": true })
    );

    // The call is answered as stopped, and the parent does not ask the model again.
    let kept = connect(&stand.socket, &format!("chat-get {id}"));
    let result = kept.iter().find(|r| r["kind"] == "toolresult").unwrap();
    assert_eq!(result["text"], "stopped by the user");
    thread::sleep(Duration::from_millis(300));
    assert_eq!(stand.bodies.lock().unwrap().len(), 2);
    // What was sent before the stop is still counted.
    let usage = stand.usage.lock().unwrap().clone();
    assert_eq!(usage.len(), 1);
    assert!(usage[0].sent > 0 && usage[0].received > 0, "{usage:?}");
}

#[test]
fn a_sub_agent_that_cannot_work_gives_an_error_result_and_the_parent_goes_on() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![
            (subagent::NAME, json!({ "task": "  " })),
            (subagent::NAME, json!({ "files": ["a.txt"] })),
            (subagent::NAME, json!({ "task": "fails at the provider" })),
            (subagent::NAME, json!({ "task": "says nothing" })),
        ]),
        Turn::Error,
        Turn::Text(""),
        Turn::Text("I saw four results."),
    ]);
    let events = stand.say("go");
    assert_eq!(events.last().unwrap(), &json!({ "event": "done" }));
    let bodies = stand.bodies();
    assert_eq!(bodies.len(), 4);
    let results: Vec<String> = messages(&bodies[3])
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["content"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        results,
        [
            "Error: the task is empty",
            "Error: the arguments are not valid for this tool",
            "Error: the sub-agent failed: error 500",
            "The sub-agent gave no answer.",
        ]
    );
}

#[test]
fn a_closed_window_ends_the_sub_agent_before_it_changes_a_file() {
    let open = Arc::new(AtomicBool::new(false));
    let stand = Stand::new(vec![
        Turn::Calls(vec![(subagent::NAME, json!({ "task": "write a note" }))]),
        Turn::Gated(
            Arc::clone(&open),
            Box::new(Turn::Calls(vec![(
                "Write",
                json!({ "file_path": "late.txt", "content": "x" }),
            )])),
        ),
        Turn::Text("never asked"),
        Turn::Text("hello again"),
    ]);
    // The window reads up to the call of the sub-agent and closes; only then the provider answers
    // the sub-agent.
    let mut client = UnixStream::connect(&stand.socket).unwrap();
    client
        .write_all(format!("chat-send {}\n", json!({ "message": "start" })).as_bytes())
        .unwrap();
    let mut seen = String::new();
    let mut reader = std::io::BufReader::new(client.try_clone().unwrap());
    while !seen.contains("Asked a sub-agent") {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0, "{seen}");
        seen.push_str(&line);
    }
    drop(reader);
    drop(client);
    open.store(true, Ordering::SeqCst);

    // The answer ends: the chat takes the next message, and nothing more was asked or written.
    let mut events = Vec::new();
    wait_for("the answer to end", || {
        events = stand.say("again");
        events.last().unwrap()["event"] == "done"
    });
    assert!(!stand.workspace.join("late.txt").exists());
    assert_eq!(stand.bodies.lock().unwrap().len(), 3);
}
