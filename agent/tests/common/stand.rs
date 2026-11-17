//! Helpers the tests of the agent's socket share: waiting, the chat's deadlines, the manager's
//! pauses, collection settings, and commands to the socket.
#![allow(clippy::unwrap_used, dead_code)]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use construct_agent::chat::Deadlines;
use construct_agent::manage::Timing;
use construct_agent::settings::{Settings, Update};
use serde_json::{Value, json};

/// Waits until `done`, 20 seconds at most.
pub fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(20));
    }
}

/// The deadlines of a chat against a local fake provider.
pub fn deadlines() -> Deadlines {
    Deadlines {
        connect: Duration::from_secs(5),
        first_word: Duration::from_secs(10),
        silence: Duration::from_secs(10),
    }
}

/// Changes the collection settings; `update` is the JSON object of the `collection` part.
pub fn set_collection(settings: &Settings, update: &str) {
    let update = format!(r#"{{"collection":{update}}}"#);
    settings.apply(Update::parse(&update).unwrap()).unwrap();
}

/// One command to the agent's socket; the JSON lines of its answer.
pub fn connect(socket: &Path, request: &str) -> Vec<Value> {
    let mut s = UnixStream::connect(socket).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    s.write_all(request.as_bytes()).unwrap();
    s.write_all(b"\n").unwrap();
    let mut text = String::new();
    s.read_to_string(&mut text).unwrap();
    text.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// The pauses of a manager in the tests: short.
pub fn timing() -> Timing {
    Timing {
        retry: [Duration::from_millis(10); 2],
        idle: Duration::from_millis(20),
        failed: Duration::from_millis(20),
        refused: Duration::from_millis(50),
        call: Duration::from_millis(600),
    }
}

/// Sends one message to the chat on `socket`; the conversation id and the events.
pub fn chat_send(socket: &Path, conversation: &str, message: &str) -> (String, Vec<Value>) {
    let events = connect(
        socket,
        &format!(
            "chat-send {}",
            json!({ "conversation": conversation, "message": message })
        ),
    );
    assert_eq!(
        events.last().unwrap(),
        &json!({ "event": "done" }),
        "{events:?}"
    );
    let id = events.iter().find(|e| e["event"] == "start").unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    (id, events)
}
