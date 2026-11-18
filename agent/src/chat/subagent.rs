//! The `run_subagent` tool: the model hands a task to a fresh conversation of the same model.
//!
//! The sub-agent starts with its own short instruction and the tools of the chat except this one,
//! works until a turn has no tool call, and its last text is the result of the call. Its
//! conversation is kept in memory for the call only: it is not stored, it is not part of the
//! parent's conversation, and its text goes to the parent's model and nowhere else. The call runs
//! inside the parent's call, so a stop of the answer ends both, and the bytes it sends and receives
//! are counted with the parent's.

use std::sync::Arc;

use connectors::{Author, Record, Role, ToolResult, Turn};
use serde::Deserialize;

use super::tool::{ChatTool, limited};
use super::wire::{Calls, request_body_with};
use super::{Chat, End, Failure, Piece, Run, SOURCE, STOPPED_RESULT, tool_calls};

/// The name the model calls the tool by.
pub const NAME: &str = "run_subagent";

/// The result of a call to this tool by a sub-agent.
const NESTED: &str = "a sub-agent cannot start another sub-agent";
/// The result when the sub-agent ends with no text.
const NO_ANSWER: &str = "The sub-agent gave no answer.";
/// Most words of the task that the status line shows.
const LINE_WORDS: usize = 8;
/// The session name of the records of a sub-agent, which are never stored.
const SESSION: &str = "sub-agent";

/// The one system instruction of a sub-agent. It never changes, because the provider's prompt cache
/// needs the same start for every request.
const INSTRUCTION: &str = "You are a helper that another assistant has handed one task. You have a workspace folder with the \
user's files, and the files are text. You work in it with `Glob`, `Read`, `Write` and `Edit`. `Read` shows every \
line with its number and a tab: the numbers are not part of the file, so never put them in the text you give to \
`Edit` or `Write`. Do the task, then answer with one final text that holds everything the other assistant needs: \
it sees nothing of your work except that text. When a tool returns an error, say what the error says, and never \
say that a change was made when it failed.";

/// The tool, to be put once in the list the chat is given, after the others.
#[must_use]
pub fn tool() -> Arc<dyn ChatTool> {
    Arc::new(SubAgent)
}

struct SubAgent;

#[derive(Deserialize)]
struct Arguments {
    task: String,
    #[serde(default)]
    files: Vec<String>,
}

impl ChatTool for SubAgent {
    fn name(&self) -> &'static str {
        NAME
    }

    fn description(&self) -> &'static str {
        "Hands a task to a sub-agent: a fresh conversation of the same model that has the file tools \
         over the workspace folder and none of this conversation. Describe the task in full, because \
         the sub-agent knows nothing else, and name the files it should look at. It works until it is \
         done, and the result is its last text. Use it for work that reads a lot, such as many files, \
         so that only the answer comes back here."
    }

    fn schema(&self) -> &'static str {
        r#"{"type":"object","properties":{"task":{"type":"string","description":"What the sub-agent must do, in words, and what its answer must hold."},"files":{"type":"array","items":{"type":"string"},"description":"Paths, relative to the workspace folder, of the files to start with."}},"required":["task"]}"#
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

fn record_turn(role: Role, text: String) -> Record {
    Record::Turn(Turn {
        tool: SOURCE.into(),
        session: SESSION.into(),
        time: String::new(),
        folder: String::new(),
        branch: String::new(),
        role,
        author: Author::Unknown,
        text,
        tools: Vec::new(),
    })
}

impl Chat {
    /// Run a sub-agent for the call with `arguments`; the text of its last turn is the result.
    /// Its requests and answers are counted in the run's usage. Its text goes nowhere but back to
    /// the model, yet each of its tool calls tells the client by its line, as the parent's do, so
    /// the user sees its file changes, and a client that is gone ends the answer: the sub-agent
    /// cancels it and stops.
    pub(super) async fn run_subagent(
        &self,
        run: &Run<'_>,
        arguments: &str,
        emit: &mut dyn FnMut(Piece<'_>) -> bool,
    ) -> Result<String, String> {
        let Run {
            target,
            cancel,
            usage,
        } = *run;
        let Ok(Arguments { task, files }) = serde_json::from_str(arguments) else {
            return Err("the arguments are not valid for this tool".into());
        };
        if task.trim().is_empty() {
            return Err("the task is empty".into());
        }
        let task = if files.is_empty() {
            task
        } else {
            format!("{task}\n\nFiles to start with: {}", files.join(", "))
        };
        let tools = &self.inner.sub_tools;
        let mut records = vec![record_turn(Role::User, task)];
        loop {
            let body = request_body_with(INSTRUCTION, &target.model, &records, tools.listing())
                .map_err(|_| "the sub-agent failed: error".to_string())?;
            usage.sent(body.len());
            let mut text = String::new();
            let mut calls = Calls::default();
            let end = self
                .stream_answer(target, body, cancel, &mut text, &mut calls, &mut |_| true)
                .await;
            usage.received(text.len());
            match end {
                End::Done => {}
                End::Stopped => return Err(STOPPED_RESULT.into()),
                End::Failed(failure) => {
                    return Err(format!("the sub-agent failed: {}", failure.reason()));
                }
            }
            let calls = calls.finish();
            if calls.is_empty() {
                return Ok(if text.is_empty() {
                    NO_ANSWER.to_string()
                } else {
                    text
                });
            }
            // A call with no name cannot be answered or replayed: the turn is broken.
            if calls.iter().any(|call| call.name.is_empty()) {
                return Err(format!("the sub-agent failed: {}", Failure::Cut.reason()));
            }
            usage.calls(&calls);
            if !text.is_empty() {
                records.push(record_turn(Role::Assistant, text));
            }
            let calls = tool_calls(calls, SESSION);
            records.extend(calls.iter().cloned().map(Record::ToolCall));
            for call in &calls {
                let result = if call.name == NAME {
                    Err(NESTED.to_string())
                } else if emit(Piece::Tool(&tools.line(&call.name, &call.arguments))) {
                    // Ending the answer drops this future, and with it the call: no cancel check
                    // of its own is needed here.
                    self.run_tool(tools, &call.name, &call.arguments).await
                } else {
                    cancel.cancel();
                    return Err(STOPPED_RESULT.into());
                };
                records.push(Record::ToolResult(ToolResult {
                    tool: SOURCE.into(),
                    session: SESSION.into(),
                    time: String::new(),
                    call_id: call.call_id.clone(),
                    text: limited(result.unwrap_or_else(|error| format!("Error: {error}"))),
                }));
            }
        }
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
