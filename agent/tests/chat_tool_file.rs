//! The tool file of the desktop against the tools of the code: every tool the chat and the setup
//! offer has an entry, and the request they make is the stored one (the one the golden test of the
//! `chat-core` crate checks over the same file).
#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::sync::Arc;

use chat_core::wire::request_body;
use construct_agent::chat::tool::{
    ChatTools, chat_list, desktop_config, setup_list, setup_tools, standard_tools,
};
use construct_agent::workspace::Workspace;

mod common;

#[path = "../../chat-core/tests/golden/conversation.rs"]
mod conversation;

use common::sessions::render_tool;

fn workspace() -> (std::path::PathBuf, Arc<Workspace>) {
    let dir = std::env::temp_dir().join(format!("construct-tool-file-{}", std::process::id()));
    let workspace = Arc::new(Workspace::open(&dir).unwrap());
    (dir, workspace)
}

fn body(tools: &ChatTools) -> String {
    request_body(
        conversation::MODEL,
        &conversation::conversation(),
        tools.listing(),
    )
    .unwrap()
}

#[test]
fn the_setup_list_makes_the_stored_request() {
    let (dir, workspace) = workspace();
    let tools = setup_list(setup_tools(&workspace, Some(render_tool(&workspace))), true).unwrap();
    let stored = include_str!("../../chat-core/tests/golden/desktop.json");
    assert_eq!(body(&tools), stored.strip_suffix('\n').unwrap_or(stored));
    // The tools carry the file's texts too, so a list built straight from them is the same.
    let direct = ChatTools::new(setup_tools(&workspace, Some(render_tool(&workspace)))).unwrap();
    assert_eq!(body(&direct), body(&tools));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_chat_list_is_the_file_entries_it_offers_in_the_file_order() {
    let (dir, workspace) = workspace();
    let with = chat_list(
        standard_tools(&workspace, Some(render_tool(&workspace))),
        true,
    )
    .unwrap();
    let names = |tools: &ChatTools| -> Vec<String> {
        let listing: serde_json::Value =
            serde_json::from_str(tools.listing().unwrap().get()).unwrap();
        listing
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(
        names(&with),
        [
            "Read",
            "Write",
            "Edit",
            "Glob",
            "render_dashboard",
            "run_subagent"
        ]
    );
    let without = chat_list(standard_tools(&workspace, None), false).unwrap();
    assert_eq!(
        names(&without),
        ["Read", "Write", "Edit", "Glob", "run_subagent"]
    );
    // The setup session without the dashboard tool has every other entry.
    let setup = setup_list(setup_tools(&workspace, None), false).unwrap();
    assert_eq!(
        names(&setup).len(),
        desktop_config().unwrap().names().len() - 1
    );
    // A list with the dashboard tool left out of the code but not of the request is refused.
    let refused = chat_list(standard_tools(&workspace, None), true)
        .err()
        .unwrap();
    assert!(refused.contains("render_dashboard"), "{refused}");
    let _ = std::fs::remove_dir_all(dir);
}
