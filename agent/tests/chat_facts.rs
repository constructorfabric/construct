//! The chat's user messages go to the facts route: a real chat over a fake provider, a manager
//! over a fake host, and one data directory between them.
#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use construct_agent::chat::tool::{ChatTool, ChatTools};
use construct_agent::chat::{Chat, Link};
use construct_agent::ipc::Server;
use construct_agent::manage::Manager;
use construct_agent::settings::{Settings, Update};
use serde_json::{Value, json};

mod common;

use common::fake_host::FakeHost;
use common::stand::{chat_send, connect, deadlines, set_collection, timing, wait_for};
use common::{done, head, raw_chunk, read_request};

const TOOL_RESULT: &str = "the lookup found nothing about editors";

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A provider that answers its first request with a call to `lookup`, and every other with a
/// short text.
fn start_provider() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let count = Arc::new(AtomicU32::new(0));
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let _ = read_request(&mut stream);
            let n = count.fetch_add(1, Ordering::SeqCst);
            thread::spawn(move || {
                head(&mut stream);
                let chunks = match n {
                    0 => vec![json!({ "choices": [{ "delta": { "tool_calls": [{
                        "index": 0, "id": "call_a", "type": "function",
                        "function": { "name": "lookup", "arguments": "{}" }
                    }] } }] })],
                    _ => vec![
                        json!({ "choices": [{ "delta": { "content": format!("Noted {n}.") } }] }),
                    ],
                };
                for chunk in chunks {
                    raw_chunk(&mut stream, &chunk);
                }
                let reason = if n == 0 { "tool_calls" } else { "stop" };
                raw_chunk(
                    &mut stream,
                    &json!({ "choices": [{ "delta": {}, "finish_reason": reason }] }),
                );
                done(&mut stream);
            });
        }
    });
    addr
}

struct Lookup;

impl ChatTool for Lookup {
    fn name(&self) -> &'static str {
        "lookup"
    }

    fn description(&self) -> &'static str {
        "A tool for the tests."
    }

    fn schema(&self) -> &'static str {
        r#"{"type":"object","properties":{}}"#
    }

    fn run(&self, _arguments: &str) -> Result<String, String> {
        Ok(TOOL_RESULT.into())
    }
}

struct Stand {
    base: PathBuf,
    data: PathBuf,
    settings: Settings,
    link: Link,
    host: FakeHost,
    manager: Manager,
    socket: PathBuf,
    /// Every record the chat told its recorder of.
    recorded: Arc<Mutex<Vec<connectors::Record>>>,
    _rt: tokio::runtime::Runtime,
}

impl Stand {
    /// A chat whose messages reach a manager that sends them to the fake host.
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!(
            "cb-chat-facts-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let (data, socket) = (base.join("data"), base.join("run").join("agent.sock"));
        let provider = start_provider();
        let settings = Settings::open(&base.join("config")).unwrap();
        let model = json!({ "model": {
            "base_url": format!("http://127.0.0.1:{}/v1", provider.port()),
            "model": "m",
            "api_key": "k",
        } });
        settings
            .apply(Update::parse(&model.to_string()).unwrap())
            .unwrap();
        let host = FakeHost::start();
        let link = Link::default();
        link.set(host.connection());
        let manager = Manager::open_with(&data, settings.clone(), link.clone(), timing()).unwrap();
        manager.spawn();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let deadlines = deadlines();
        let recorded: Arc<Mutex<Vec<connectors::Record>>> = Arc::default();
        let tell: construct_agent::records::Recorder = {
            let seen = recorded.clone();
            Arc::new(move |record| seen.lock().unwrap().push(record.clone()))
        };
        // The chat has the link the shipped one has, for its facts tool; its own loop sends nothing
        // to the route that takes prompts.
        let chat = Chat::new(settings.clone(), &data, rt.handle().clone(), deadlines)
            .unwrap()
            .with_link(link.clone())
            .with_tools(ChatTools::new(vec![Arc::new(Lookup)]).unwrap())
            .with_observer(manager.observer())
            .with_recorder(tell);
        let server = Server::bind(&socket)
            .unwrap()
            .with_settings(settings.clone())
            .with_chat(chat);
        thread::spawn(move || server.serve());
        Self {
            base,
            data,
            settings,
            link,
            host,
            manager,
            socket,
            recorded,
            _rt: rt,
        }
    }

    fn connect(&self, request: &str) -> Vec<Value> {
        connect(&self.socket, request)
    }

    /// Sends one message; the conversation id.
    fn send(&self, conversation: &str, message: &str) -> String {
        chat_send(&self.socket, conversation, message).0
    }

    /// The stored records of a conversation.
    fn records(&self, conversation: &str) -> Vec<Value> {
        self.connect(&format!("chat-get {conversation}"))
    }

    /// A second manager on the same files, as after a restart of the agent.
    fn restart(&self) {
        Manager::open_with(
            &self.data,
            self.settings.clone(),
            self.link.clone(),
            timing(),
        )
        .unwrap()
        .spawn();
    }

    /// The bodies of the calls to the route that takes prompts; the chat's own reads of the facts
    /// go to another route.
    fn bodies(&self) -> Vec<Value> {
        self.host
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.method == "POST" && c.path.ends_with("/facts/manage"))
            .map(|c| c.body.clone())
            .collect()
    }

    /// Waits for `n` calls to the route that takes prompts.
    fn wait_for_calls(&self, n: usize) {
        wait_for("the call", || self.bodies().len() >= n);
    }

    /// A quiet moment, for a check that nothing else is sent.
    fn settle() {
        thread::sleep(Duration::from_millis(300));
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

#[test]
fn each_user_message_goes_out_once_with_the_text_before_it_and_no_tool_text() {
    let stand = Stand::new();
    let conversation = stand.send("", "I mostly write Rust");
    stand.wait_for_calls(1);
    stand.send(&conversation, "I use Postgres at work");
    stand.wait_for_calls(2);
    Stand::settle();

    let calls = stand.bodies();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[0]["message"], "I mostly write Rust");
    assert_eq!(calls[0]["conversation"], "");
    assert_eq!(calls[1]["message"], "I use Postgres at work");
    // The first answer was a tool call and then text: the call and its result are not context.
    assert_eq!(
        calls[1]["conversation"],
        "user: I mostly write Rust\nassistant: Noted 1."
    );
    let all = serde_json::to_string(&calls).unwrap();
    assert!(
        !all.contains(TOOL_RESULT) && !all.contains("lookup"),
        "{all}"
    );
}

#[test]
fn nothing_of_the_chat_is_sent_while_collection_is_off_and_the_chat_still_keeps_its_history() {
    let stand = Stand::new();
    set_collection(&stand.settings, r#"{"enabled":false}"#);
    let conversation = stand.send("", "I mostly write Rust");
    Stand::settle();
    assert!(stand.bodies().is_empty());
    let records = stand.records(&conversation);
    // The user message, the calls with their results, and the answer.
    let kinds = kinds(&records);
    assert_eq!(
        (kinds.first(), kinds.last()),
        (Some(&"turn"), Some(&"turn"))
    );
    assert!(
        kinds.contains(&"toolcall") && kinds.contains(&"toolresult"),
        "{kinds:?}"
    );

    // Turned on again, the message typed in the meantime is not sent, not even after a restart.
    set_collection(&stand.settings, r#"{"enabled":true}"#);
    stand.restart();
    Stand::settle();
    assert!(stand.bodies().is_empty());
    // A new message is sent.
    stand.send(&conversation, "I use Postgres at work");
    stand.wait_for_calls(1);
    assert_eq!(stand.bodies()[0]["message"], "I use Postgres at work");
}

#[test]
fn the_chat_follows_the_master_switch_only() {
    let stand = Stand::new();
    set_collection(&stand.settings, r#"{"claude_code":false,"codex":false}"#);
    stand.send("", "I mostly write Rust");
    stand.wait_for_calls(1);
}

#[test]
fn a_restart_does_not_send_a_message_again() {
    let stand = Stand::new();
    stand.send("", "I mostly write Rust");
    stand.wait_for_calls(1);
    stand.restart();
    Stand::settle();
    assert_eq!(stand.bodies().len(), 1);
}

#[test]
fn a_host_that_fails_changes_nothing_in_the_chat() {
    let stand = Stand::new();
    for _ in 0..12 {
        stand.host.answer(503, "{}");
    }
    let conversation = stand.send("", "I mostly write Rust");
    let before = stand.records(&conversation);
    // The calls have failed and the manager has given the message up.
    wait_for("the failure", || {
        stand.manager.status()["live"]["failed"] == 1
    });
    assert_eq!(stand.records(&conversation), before);
    // The chat goes on answering, and what was stored stays as it was.
    stand.send(&conversation, "I use Postgres at work");
    let after = stand.records(&conversation);
    assert_eq!(after[..before.len()], before[..]);
    assert!(after.len() > before.len());
}

#[test]
fn the_chat_tells_its_recorder_of_each_record_it_stores() {
    let stand = Stand::new();
    stand.send("", "I mostly write Rust");
    stand.wait_for_calls(1);
    let recorded = stand.recorded.lock().unwrap().clone();
    // The user's message, the tool call and its result, and the answer; the chat's own view of
    // the conversation adds a line for the tool that is no record.
    let kinds_recorded: Vec<&str> = recorded
        .iter()
        .map(|r| match r {
            connectors::Record::Turn(t) if t.role == connectors::Role::User => "user",
            connectors::Record::Turn(_) => "assistant",
            connectors::Record::Mark(_) => "mark",
            connectors::Record::ToolCall(_) => "tool_call",
            connectors::Record::ToolResult(_) => "tool_result",
        })
        .collect();
    assert_eq!(
        kinds_recorded,
        ["user", "tool_call", "tool_result", "assistant"]
    );
}

fn kinds(records: &[Value]) -> Vec<&str> {
    records
        .iter()
        .map(|r| r["kind"].as_str().unwrap())
        .collect()
}
