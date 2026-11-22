//! The request body of a fixed conversation with a `recall_facts` pair, built over the tool file of
//! the phone and over the tool file of the desktop, against the files that `origin/main` produced
//! before the chat core moved out of the agent. The bytes are what the provider's prompt cache
//! sees, so they may not change.
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::sync::Arc;

use chat_core::tool::{ChatTools, ToolConfig, ToolHandler};
use chat_core::wire::request_body;

#[path = "golden/conversation.rs"]
mod conversation;

/// A handler that is never run: the body only has the list.
struct Idle(String);

impl ToolHandler for Idle {
    fn name(&self) -> &str {
        &self.0
    }

    fn run(&self, _: &str) -> Result<String, String> {
        Ok(String::new())
    }
}

/// A stored body without the newline that the end-of-file check of the repository puts at the end
/// of a file: the body itself has none.
fn stored(file: &str) -> &str {
    file.strip_suffix('\n').unwrap_or(file)
}

fn body_over(file: &str) -> String {
    let config = ToolConfig::parse(file).unwrap();
    let handlers: Vec<Arc<dyn ToolHandler>> = config
        .names()
        .into_iter()
        .map(|name| Arc::new(Idle(name.to_string())) as Arc<dyn ToolHandler>)
        .collect();
    let tools = ChatTools::configured(&config, &handlers).unwrap();
    request_body(
        conversation::MODEL,
        &conversation::conversation(),
        tools.listing(),
    )
    .unwrap()
}

#[test]
fn the_phone_list_gives_the_body_it_gave_before() {
    let body = body_over(include_str!("../../tools/phone.json"));
    assert_eq!(body, stored(include_str!("golden/phone.json")));
    // The phone offers `recall_facts` and nothing else.
    assert_eq!(body.matches("\"parameters\":").count(), 1);
}

#[test]
fn the_desktop_list_gives_the_body_it_gave_before() {
    let body = body_over(include_str!("../../tools/desktop.json"));
    assert_eq!(body, stored(include_str!("golden/desktop.json")));
}
