//! The `run_subagent` tool of the desktop chat: its arguments and the line it shows. The call itself
//! runs in the loop of `chat-core` (see its `subagent` module), which needs the model and the stop.

use std::sync::Arc;

use serde::Deserialize;

pub use chat_core::subagent::NAME;
pub(super) use chat_core::subagent_instruction as instruction;

use super::tool::ChatTool;

/// Most words of the task that the status line shows.
const LINE_WORDS: usize = 8;

/// The tool, to be put once in the list the chat is given, after the others.
#[must_use]
pub fn tool() -> Arc<dyn ChatTool> {
    Arc::new(SubAgent)
}

struct SubAgent;

#[derive(Deserialize)]
struct Arguments {
    task: String,
}

impl ChatTool for SubAgent {
    fn name(&self) -> &'static str {
        NAME
    }

    fn description(&self) -> &'static str {
        super::tool::description(self.name())
    }

    fn schema(&self) -> &'static str {
        super::tool::schema(self.name())
    }

    fn line(&self, arguments: &str) -> String {
        const SHOWN: &str = "Asked a sub-agent";
        let Ok(arguments) = serde_json::from_str::<Arguments>(arguments) else {
            return SHOWN.to_string();
        };
        let mut words = arguments.task.split_whitespace();
        let first: Vec<&str> = words.by_ref().take(LINE_WORDS).collect();
        if first.is_empty() {
            return SHOWN.to_string();
        }
        let more = if words.next().is_some() { "…" } else { "" };
        format!("{SHOWN}: {}{more}", first.join(" "))
    }

    /// The chat runs this tool itself, because it needs the model and the stop; the list only
    /// offers it.
    fn run(&self, _: &str) -> Result<String, String> {
        Err("this tool runs only in the chat".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_names_the_first_words_of_the_task_and_never_the_files() {
        let tool = SubAgent;
        assert_eq!(
            tool.line(r#"{"task":"  read   the notes\nand list them ","files":["secret.txt"]}"#),
            "Asked a sub-agent: read the notes and list them"
        );
        assert_eq!(
            tool.line(r#"{"task":"one two three four five six seven eight nine"}"#),
            "Asked a sub-agent: one two three four five six seven eight…"
        );
        assert_eq!(tool.line(r#"{"task":"  "}"#), "Asked a sub-agent");
        assert_eq!(tool.line("{not json"), "Asked a sub-agent");
    }
}
