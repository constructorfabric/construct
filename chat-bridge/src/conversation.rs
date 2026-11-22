//! The fixed conversation of the golden tests and of the phone apps build checks: a user turn, the `recall_facts` pair, a tool call and
//! its result, an answer, and a second user turn. It has accents, quotes and a newline, so the
//! escapes of the body are part of what is compared.

use connectors::{Author, Record, Role, ToolCall, ToolResult, Turn};

pub const MODEL: &str = "golden-model";

fn turn(role: Role, text: &str) -> Record {
    Record::Turn(Turn {
        tool: "chat".into(),
        session: "golden".into(),
        time: String::new(),
        folder: String::new(),
        branch: String::new(),
        role,
        author: Author::Unknown,
        text: text.into(),
        tools: Vec::new(),
    })
}

fn call(id: &str, name: &str, arguments: &str) -> Record {
    Record::ToolCall(ToolCall {
        tool: "chat".into(),
        session: "golden".into(),
        time: String::new(),
        call_id: id.into(),
        name: name.into(),
        arguments: arguments.into(),
    })
}

fn result(id: &str, text: &str) -> Record {
    Record::ToolResult(ToolResult {
        tool: "chat".into(),
        session: "golden".into(),
        time: String::new(),
        call_id: id.into(),
        text: text.into(),
    })
}

#[must_use]
pub fn conversation() -> Vec<Record> {
    vec![
        turn(Role::User, "What do I know about \"Ünï\" today?"),
        call("call_golden_recall", "recall_facts", "{}"),
        result(
            "call_golden_recall",
            "These are the facts known about the user. This list replaces all earlier lists of facts.\nperson.name: Ada\nperson.city: Zürich",
        ),
        call(
            "call_golden_read",
            "Read",
            r#"{"offset":1,"file_path":"notes.txt"}"#,
        ),
        result("call_golden_read", "     1\tbuy milk\n     2\tcall Ada"),
        turn(Role::Assistant, "Hello Ada.\nYour notes say: buy milk."),
        turn(Role::User, "Thanks, and tomorrow?"),
    ]
}
