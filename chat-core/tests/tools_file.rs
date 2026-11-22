//! The tool file: what it gives the chat, and what the host must give back. A tool that exists only
//! as an entry of a file and a handler runs through the loop with no change in the crate.
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::sync::{Arc, Mutex};
use std::time::Duration;

use chat_core::model::ModelSource;
use chat_core::tool::{ChatTools, ConfigError, ToolConfig, ToolHandler};
use chat_core::{Chat, Deadlines};
use connectors::Record;
use serde_json::{Value, json};

mod common;

use common::{Fake, Fixed, Memory, Reply, model_at};

/// A handler that answers every call with `answer`, and keeps the arguments it was given.
struct Answering {
    name: &'static str,
    answer: &'static str,
    calls: Arc<Mutex<Vec<String>>>,
}

impl ToolHandler for Answering {
    fn name(&self) -> &str {
        self.name
    }

    fn line(&self, arguments: &str) -> String {
        let city = serde_json::from_str::<Value>(arguments)
            .ok()
            .and_then(|v| v["city"].as_str().map(str::to_owned));
        city.map_or_else(|| self.name.to_string(), |city| format!("Looked up {city}"))
    }

    fn run(&self, arguments: &str) -> Result<String, String> {
        self.calls.lock().unwrap().push(arguments.to_string());
        Ok(self.answer.to_string())
    }
}

/// A file with one tool the crate has never heard of: written only here.
const WEATHER_FILE: &str = r#"[
  {
    "name": "weather",
    "description": "The weather in a city, in a few words.",
    "schema": {"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}
  }
]"#;

fn chunk(delta: &Value, finish: Option<&str>) -> String {
    json!({ "choices": [{ "delta": delta, "finish_reason": finish }] }).to_string()
}

#[test]
fn a_tool_that_is_only_in_the_file_is_offered_and_runs_through_the_loop() {
    let provider = Fake::start(vec![
        Reply::Events(vec![chunk(
            &json!({ "tool_calls": [{ "index": 0, "id": "call_1",
                "function": { "name": "weather", "arguments": "{\"city\":\"Zürich\"}" } }] }),
            Some("tool_calls"),
        )]),
        Reply::Events(vec![chunk(&json!({ "content": "Sunny." }), Some("stop"))]),
    ]);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let handler: Arc<dyn ToolHandler> = Arc::new(Answering {
        name: "weather",
        answer: "sunny, 21 degrees",
        calls: Arc::clone(&calls),
    });
    let tools =
        ChatTools::configured(&ToolConfig::parse(WEATHER_FILE).unwrap(), &[handler]).unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let memory = Memory::default();
    let source: Arc<dyn ModelSource> = Arc::new(Fixed(model_at(provider.addr)));
    let chat = Chat::new(
        source,
        Box::new(memory.clone()),
        rt.handle().clone(),
        Deadlines {
            connect: Duration::from_secs(5),
            first_word: Duration::from_secs(10),
            silence: Duration::from_secs(10),
        },
    )
    .unwrap()
    .with_tools(tools);
    let mut lines = Vec::new();
    chat.send(r#"{"message":"weather in Zürich?"}"#, &mut |line| {
        lines.push(line.to_string());
        true
    });

    // The tool is in the first request, as the file wrote it.
    let bodies = provider.bodies.lock().unwrap();
    assert_eq!(bodies.len(), 2, "{lines:?}");
    assert!(bodies[0].contains(
        r#""tools":[{"type":"function","function":{"name":"weather","description":"The weather in a city, in a few words.","parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}}}]"#
    ), "{}", bodies[0]);
    // The handler ran with the arguments as the model wrote them, and its answer went back.
    assert_eq!(*calls.lock().unwrap(), [r#"{"city":"Zürich"}"#]);
    assert!(
        bodies[1].contains(r#""content":"sunny, 21 degrees""#),
        "{}",
        bodies[1]
    );
    // The window was told the handler's line, and the answer ended well.
    assert!(
        lines
            .iter()
            .any(|l| l.contains(r#""line":"Looked up Zürich""#)),
        "{lines:?}"
    );
    assert_eq!(lines.last().unwrap(), r#"{"event":"done"}"#);
    let stored: Vec<_> = memory
        .0
        .lock()
        .unwrap()
        .iter()
        .filter_map(|r| match r {
            Record::ToolResult(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect();
    assert!(
        stored.contains(&"sunny, 21 degrees".to_string()),
        "{stored:?}"
    );
}

struct Named(&'static str);

impl ToolHandler for Named {
    fn name(&self) -> &str {
        self.0
    }

    fn run(&self, _: &str) -> Result<String, String> {
        Ok(String::new())
    }
}

fn handlers(names: &[&'static str]) -> Vec<Arc<dyn ToolHandler>> {
    names
        .iter()
        .map(|&n| Arc::new(Named(n)) as Arc<dyn ToolHandler>)
        .collect()
}

const TWO_FILE: &str = r#"[
  {"name":"first","description":"One.","schema":{}},
  {"name":"second","description":"Two.","schema":{}}
]"#;

#[test]
fn a_file_that_lists_a_tool_with_no_handler_fails_with_a_plain_error() {
    let config = ToolConfig::parse(TWO_FILE).unwrap();
    let error = ChatTools::configured(&config, &handlers(&["first"]))
        .err()
        .unwrap();
    assert_eq!(error, ConfigError::NoHandler("second".into()));
    assert_eq!(
        error.to_string(),
        "the tool file lists `second`, and the host has no handler for it"
    );
}

#[test]
fn a_handler_with_no_entry_is_not_offered_and_the_file_gives_the_order() {
    let config = ToolConfig::parse(TWO_FILE).unwrap();
    let tools = ChatTools::configured(&config, &handlers(&["extra", "second", "first"])).unwrap();
    let listing = tools.listing().unwrap().get().to_string();
    assert!(!listing.contains("extra"), "{listing}");
    let (first, second) = (
        listing.find("\"first\"").unwrap(),
        listing.find("\"second\"").unwrap(),
    );
    assert!(first < second, "{listing}");
    // A host that serves only some of a file's tools says which.
    let some =
        ChatTools::configured(&config.only(&["second"]), &handlers(&["first", "second"])).unwrap();
    assert!(!some.listing().unwrap().get().contains("first"));
}

#[test]
fn a_file_that_a_provider_would_refuse_is_not_taken() {
    for (text, error) in [
        ("not json", ConfigError::NotATool),
        (r#"{"name":"a"}"#, ConfigError::NotATool),
        (
            r#"[{"name":"a","description":"d","schema":{},"extra":1}]"#,
            ConfigError::NotATool,
        ),
        (
            r#"[{"name":"","description":"d","schema":{}}]"#,
            ConfigError::Invalid(String::new(), "the name is empty"),
        ),
        (
            r#"[{"name":"a","description":"d","schema":{}},{"name":"a","description":"d","schema":{}}]"#,
            ConfigError::Invalid("a".into(), "another tool has the name"),
        ),
        (
            r#"[{"name":"a","description":"d","schema":[]}]"#,
            ConfigError::Invalid("a".into(), "the schema is not a JSON object"),
        ),
    ] {
        assert_eq!(ToolConfig::parse(text).err().unwrap(), error, "{text}");
    }
    let config = ToolConfig::parse(TWO_FILE).unwrap();
    let twice = handlers(&["first", "first", "second"]);
    assert_eq!(
        ChatTools::configured(&config, &twice).err().unwrap(),
        ConfigError::TwoHandlers("first".into())
    );
}

#[test]
fn a_schema_keeps_its_spaces_and_escapes_as_the_file_wrote_them() {
    let file = r#"[{"name":"a","description":"A \"quoted\" word, é and é.","schema":{ "type" : "object",
 "properties":{"p":{"description":"xé\n"}} }}]"#;
    let config = ToolConfig::parse(file).unwrap();
    let tools = ChatTools::configured(&config, &handlers(&["a"])).unwrap();
    let listing = tools.listing().unwrap().get().to_string();
    // The schema text goes out byte for byte; the description is written the way the crate writes
    // every string.
    assert!(
        listing.contains(
            r#""parameters":{ "type" : "object",
 "properties":{"p":{"description":"xé\n"}} }"#
        ),
        "{listing}"
    );
    assert!(
        listing.contains(r#""description":"A \"quoted\" word, é and é.""#),
        "{listing}"
    );
}
