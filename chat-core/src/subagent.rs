//! The loop's part of the `run_subagent` tool: the model hands a task to a fresh conversation of the
//! same model.
//!
//! The sub-agent starts with its own short instruction and the tools of the chat except this one,
//! works until a turn has no tool call, and its last text is the result of the call. Its
//! conversation is kept in memory for the call only: it is not stored, it is not part of the
//! parent's conversation, and its text goes to the parent's model and nowhere else. The call runs
//! inside the parent's call, so a stop of the answer ends both, and the bytes it sends and receives
//! are counted with the parent's.
//!
//! The tool itself (its arguments, the line it shows) is the host's: the chat only knows its name,
//! [`NAME`], and runs a call to it here instead of on a thread of its own.

use connectors::{Author, Record, Role, ToolResult, Turn};
use serde::Deserialize;

use crate::chat::{Chat, End, Failure, Piece, Run, SOURCE, STOPPED_RESULT, tool_calls};
use crate::tool::{ChatTools, limited};
use crate::wire::{Calls, request_body_with};

/// The name the model calls the tool by.
pub const NAME: &str = "run_subagent";

/// The result of a call to this tool by a sub-agent.
const NESTED: &str = "a sub-agent cannot start another sub-agent";
/// The result when the sub-agent ends with no text.
const NO_ANSWER: &str = "The sub-agent gave no answer.";
/// The session name of the records of a sub-agent, which are never stored.
const SESSION: &str = "sub-agent";

/// The text of the one system instruction of a sub-agent, as a macro so that an instruction can be
/// joined to it when it is compiled.
#[macro_export]
macro_rules! subagent_instruction {
    () => {
        "You are a helper that another assistant has handed one task. You have a workspace folder with the \
user's files, and the files are text. You work in it with `Glob`, `Read`, `Write` and `Edit`. `Read` shows every \
line with its number and a tab: the numbers are not part of the file, so never put them in the text you give to \
`Edit` or `Write`. Do the task, then answer with one final text that holds everything the other assistant needs: \
it sees nothing of your work except that text. When a tool returns an error, say what the error says, and never \
say that a change was made when it failed."
    };
}

/// The one system instruction of a sub-agent. It never changes, because the provider's prompt cache
/// needs the same start for every request.
pub const INSTRUCTION: &str = subagent_instruction!();

#[derive(Deserialize)]
struct Arguments {
    task: String,
    #[serde(default)]
    files: Vec<String>,
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

/// Why a conversation ended before the model gave its last text.
#[derive(Debug, PartialEq, Eq)]
pub enum Halt {
    /// By a stop, or because the client is gone.
    Stopped,
    Failed(Failure),
}

impl From<End> for Halt {
    fn from(end: End) -> Self {
        match end {
            End::Failed(failure) => Self::Failed(failure),
            // `stream_answer` ends `Done` only when the answer is whole, which callers handle first.
            End::Done | End::Stopped => Self::Stopped,
        }
    }
}

impl Chat {
    /// Run a sub-agent for the call with `arguments`, with `instruction` and the tools `sub`; the
    /// text of its last turn is the result. Its requests and answers are counted in the run's
    /// usage. Its text goes nowhere but back to the model, yet each of its tool calls tells the
    /// client by its line, as the parent's do, so the user sees its file changes, and a client that
    /// is gone ends the answer: the sub-agent cancels it and stops.
    pub(crate) async fn run_subagent(
        &self,
        run: &Run<'_>,
        arguments: &str,
        (instruction, sub): (&str, &ChatTools),
        emit: &mut dyn FnMut(Piece<'_>) -> bool,
    ) -> Result<String, String> {
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
        // The sub-agent has the tools of the list but the sub-agent tool, so the two functions
        // call each other one level deep only.
        Box::pin(self.converse(run, (instruction, sub), None, task, emit))
            .await
            .map_err(|halt| match halt {
                Halt::Failed(failure) => format!("the sub-agent failed: {}", failure.reason()),
                Halt::Stopped => STOPPED_RESULT.into(),
            })
    }

    /// A conversation of the model that is kept in memory only: it starts with the user turn
    /// `start`, runs the model with the instruction and tools of `own`, and answers the calls it
    /// asks for until a turn has none. The text of that turn is the result. A call to the
    /// sub-agent tool runs a sub-agent with `nested` (an instruction and tools), and is refused
    /// when there is none. The tool lines go to `emit`; a client that is gone cancels the run and
    /// ends it as a stop does.
    ///
    /// # Errors
    /// Why the conversation ended before the model gave its last text.
    pub async fn converse(
        &self,
        run: &Run<'_>,
        (instruction, tools): (&str, &ChatTools),
        nested: Option<(&str, &ChatTools)>,
        start: String,
        emit: &mut dyn FnMut(Piece<'_>) -> bool,
    ) -> Result<String, Halt> {
        let Run {
            target,
            cancel,
            usage,
            ..
        } = *run;
        let mut records = vec![record_turn(Role::User, start)];
        loop {
            let body = request_body_with(instruction, &target.model, &records, tools.listing())
                .map_err(|_| Halt::Failed(Failure::Internal))?;
            usage.sent(body.len());
            let mut text = String::new();
            let mut calls = Calls::default();
            let end = self
                .stream_answer(target, body, cancel, &mut text, &mut calls, &mut |_| true)
                .await;
            usage.received(text.len());
            if end != End::Done {
                return Err(end.into());
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
                return Err(Halt::Failed(Failure::Cut));
            }
            usage.calls(&calls);
            if !text.is_empty() {
                records.push(record_turn(Role::Assistant, text));
            }
            let calls = tool_calls(calls, SESSION);
            records.extend(calls.iter().cloned().map(Record::ToolCall));
            for call in &calls {
                // A sub-agent cannot start another: the call is refused before the client is told.
                let refused = call.name == NAME && nested.is_none();
                if !refused && !emit(Piece::Tool(&tools.line(&call.name, &call.arguments))) {
                    cancel.cancel();
                    return Err(Halt::Stopped);
                }
                // Ending the answer drops this future, and with it the call: no cancel check of
                // its own is needed here.
                let result = match nested {
                    _ if refused => Err(NESTED.to_string()),
                    Some(nested) if call.name == NAME => {
                        self.run_subagent(run, &call.arguments, nested, emit).await
                    }
                    _ => {
                        self.run_tool(run.tools, tools, &call.name, &call.arguments)
                            .await
                    }
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
