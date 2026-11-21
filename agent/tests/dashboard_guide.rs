//! The guide files for the chat: written at every start, kept current, and true to the type.

#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::path::PathBuf;
use std::sync::Arc;

use construct_agent::chat::INSTRUCTION;
use construct_agent::chat::dashboard_view;
use construct_agent::dashboard::{document_schema, validate_activity};
use construct_agent::dashboard_document::Dashboard;
use construct_agent::dashboard_guide::{GUIDE, GUIDE_FILE, SCHEMA_FILE, write};
use construct_agent::workspace::Workspace;
use serde_json::Value;

fn workspace(name: &str) -> (Workspace, PathBuf) {
    let root = std::env::temp_dir().join(format!("cb-guide-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    (Workspace::open(&root).unwrap(), root)
}

/// The text of every fenced `json` block of the guide.
fn examples(guide: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut open: Option<String> = None;
    for line in guide.lines() {
        match (&mut open, line.trim_end()) {
            (None, "```json") => open = Some(String::new()),
            (Some(_), "```") => found.extend(open.take()),
            (Some(text), _) => {
                text.push_str(line);
                text.push('\n');
            }
            (None, _) => {}
        }
    }
    found
}

#[test]
fn both_files_are_written_with_the_current_content() {
    let (workspace, root) = workspace("written");
    write(&workspace).unwrap();
    let folder = root.join("workspace");
    assert_eq!(
        std::fs::read_to_string(folder.join(GUIDE_FILE)).unwrap(),
        GUIDE
    );
    let schema: Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join(SCHEMA_FILE)).unwrap()).unwrap();
    assert_eq!(schema, document_schema());
}

/// The schema file as the model reads it, compiled: it accepts every example of the guide and
/// refuses documents that are not dashboards.
#[test]
fn the_written_schema_file_describes_the_dashboard() {
    let (workspace, root) = workspace("describes");
    write(&workspace).unwrap();
    let text = std::fs::read_to_string(root.join("workspace").join(SCHEMA_FILE)).unwrap();
    let validator =
        jsonschema::draft7::new(&serde_json::from_str::<Value>(&text).unwrap()).unwrap();

    let found = examples(GUIDE);
    assert!(!found.is_empty());
    for example in found {
        let document: Value = serde_json::from_str(&example).unwrap();
        assert!(validator.is_valid(&document), "refused: {example}");
        let mut no_title = document.clone();
        no_title.as_object_mut().unwrap().remove("title");
        assert!(!validator.is_valid(&no_title));
        let mut wide = document;
        wide["payload"]["widgets"][0]["position"]["width"] = serde_json::json!(13);
        assert!(!validator.is_valid(&wide));
    }
    assert!(!validator.is_valid(&serde_json::json!({})));
}

#[test]
fn a_start_rewrites_what_the_model_changed_or_removed() {
    let (workspace, root) = workspace("rewritten");
    let folder = root.join("workspace");
    write(&workspace).unwrap();
    workspace.write(GUIDE_FILE, "changed").unwrap();
    workspace.write(SCHEMA_FILE, "{}").unwrap();
    write(&workspace).unwrap();
    assert_eq!(
        std::fs::read_to_string(folder.join(GUIDE_FILE)).unwrap(),
        GUIDE
    );
    let schema: Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join(SCHEMA_FILE)).unwrap()).unwrap();
    assert_eq!(schema, document_schema());

    std::fs::remove_file(folder.join(GUIDE_FILE)).unwrap();
    write(&workspace).unwrap();
    assert_eq!(
        std::fs::read_to_string(folder.join(GUIDE_FILE)).unwrap(),
        GUIDE
    );
}

#[test]
fn every_json_example_of_the_guide_is_a_valid_dashboard() {
    let found = examples(GUIDE);
    assert!(found.len() >= 2, "the guide has examples");
    for text in found {
        let document: Value = serde_json::from_str(&text).unwrap();
        if let Err(violation) = validate_activity(&document) {
            panic!("an example of the guide is refused: {violation}\n{text}");
        }
    }
}

/// The first lines tell the model the person does not see the file, before any detail of it.
#[test]
fn the_guide_begins_with_the_rule_that_the_file_is_not_for_the_person() {
    let (workspace, root) = workspace("rule");
    write(&workspace).unwrap();
    let written = std::fs::read_to_string(root.join("workspace").join(GUIDE_FILE)).unwrap();
    let opening: String = written.lines().take(8).collect::<Vec<_>>().join("\n");
    assert!(opening.starts_with("# The dashboard"));
    for phrase in [
        "does not edit the dashboard file",
        "yourself with your tools",
        "say so in plain words",
        "unless they ask to see",
    ] {
        assert!(opening.contains(phrase), "the opening says: {phrase}");
    }
    assert!(
        !opening.contains("```"),
        "the rule comes before any example"
    );
}

#[test]
fn the_guide_stays_short() {
    assert!(
        GUIDE.lines().count() <= 90,
        "the model reads it when it needs it"
    );
}

#[test]
fn the_tool_names_both_files_and_the_instruction_does_not_change() {
    let (workspace, _) = workspace("tool");
    let dashboard = Dashboard::open(Arc::new(workspace));
    let tool = dashboard_view::tool(
        dashboard,
        Arc::new(|_| Ok(construct_agent::dashboard_numbers::Data::default())),
        dashboard_view::Calendar::default(),
    );
    let description = construct_agent::chat::tool::ChatTool::description(&*tool);
    assert!(description.contains(SCHEMA_FILE));
    assert!(description.contains(GUIDE_FILE));

    assert_eq!(
        INSTRUCTION,
        "You are the chat of a person's Construct, the assistant that lives in their Construct Browser. \
You cannot save anything yourself. Whatever the user tells you is remembered later by the system, on its own, \
unless the `recall_facts` tool returns the error that memory is off. So when the user tells you something worth \
keeping, say that you will remember it, and never say that you have saved it. If `recall_facts` returns the error \
that memory is off, say that nothing is remembered. \
You have a workspace folder with the user's files, and the files are text. You work in it with `Glob`, `Read`, \
`Write` and `Edit`. `Read` shows every line with its number and a tab: the numbers are not part of the file, so \
never put them in the text you give to `Edit` or `Write`. When a tool returns an error, say what the error says, \
and never say that a change was made when it failed. Answer in the language of the user."
    );
}

#[test]
fn the_guide_and_the_schema_name_the_source_and_the_groupings() {
    for word in [
        "ai_sessions",
        "on_screen",
        "`app`",
        "`site`",
        "`tool`",
        "`collection`",
        "No site",
    ] {
        assert!(GUIDE.contains(word), "the guide names {word}");
    }
    let schema = document_schema().to_string();
    for word in [
        "\"ai_sessions\"",
        "\"on_screen\"",
        "\"app\"",
        "\"site\"",
        "\"tool\"",
    ] {
        assert!(schema.contains(word), "the schema names {word}");
    }
}
