//! The tools the model may call: a trait, and the fixed list the chat offers.
//!
//! The list is part of the request the provider caches, so it is built once, in the order the
//! tools were given, from text that never changes: no time, no random order, no map. A new tool is
//! a change in the code; there is no setting for it.

use std::fmt;
use std::sync::Arc;

use serde::Serialize;
use serde_json::value::RawValue;

use super::recall::VALUE_CUT as CUT_NOTE;

/// Longest result of a call, in bytes, the note included. A longer one is cut and ends with
/// [`CUT_NOTE`].
pub(super) const MAX_RESULT: usize = 64 * 1024;

/// The line of a call to a name that is no tool of the list: the name is whatever the model wrote,
/// so it is not shown.
const UNKNOWN_LINE: &str = "unknown tool";

/// Most characters of the line a call shows. A longer line is cut and ends with an ellipsis.
const MAX_LINE: usize = 80;

/// One tool the model may call.
pub trait ChatTool: Send + Sync {
    /// The name the model calls it by.
    fn name(&self) -> &str;

    /// What the tool does and when to use it, in plain English for the model.
    fn description(&self) -> &str;

    /// The JSON Schema of the arguments, as a constant text written by hand.
    fn schema(&self) -> &str;

    /// The short line the window shows for a call, for example `Read notes.txt`: what the tool does
    /// and to what, from the arguments text as the model wrote it. It never holds content (a file's
    /// text, a search's results). When the arguments cannot be read it is the name of the tool,
    /// which is what this gives unless a tool says more.
    fn line(&self, arguments: &str) -> String {
        let _ = arguments;
        self.name().to_string()
    }

    /// Run the call with the arguments text as the model wrote it. It may block: the chat runs it
    /// on a thread of its own. The error text is given to the model as the result.
    ///
    /// # Errors
    /// With a text that tells the model what went wrong.
    fn run(&self, arguments: &str) -> Result<String, String>;
}

/// A tool the list cannot hold.
#[derive(Debug)]
pub struct InvalidTool {
    pub name: String,
    pub reason: &'static str,
}

impl fmt::Display for InvalidTool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tool `{}`: {}", self.name, self.reason)
    }
}

impl std::error::Error for InvalidTool {}

/// The tools in a fixed order, which is the order of the list the model is given. Cheap to clone.
#[derive(Clone, Default)]
pub struct ChatTools {
    tools: Vec<Arc<dyn ChatTool>>,
    /// The `tools` array of the request, written once; `None` for an empty list.
    listing: Option<Arc<RawValue>>,
}

#[derive(Serialize)]
struct Listed<'a> {
    r#type: &'static str,
    function: Described<'a>,
}

#[derive(Serialize)]
struct Described<'a> {
    name: &'a str,
    description: &'a str,
    /// The schema text, byte for byte.
    parameters: &'a RawValue,
}

/// The tools of a chat over `workspace`, in the order the model is given them: the file tools, the
/// tool that renders the dashboard when there is one, and the sub-agent tool last.
#[must_use]
pub fn standard_tools(
    workspace: &Arc<crate::workspace::Workspace>,
    render_tool: Option<Arc<dyn ChatTool>>,
) -> Vec<Arc<dyn ChatTool>> {
    listed(workspace, false, render_tool)
}

/// The tools of the setup session over `workspace`: those of [`standard_tools`] with the tools that
/// read the user's machine after the file tools, and the file tools unable to change the folder
/// list those tools take their folders from.
#[must_use]
pub fn setup_tools(
    workspace: &Arc<crate::workspace::Workspace>,
    render_tool: Option<Arc<dyn ChatTool>>,
) -> Vec<Arc<dyn ChatTool>> {
    listed(workspace, true, render_tool)
}

fn listed(
    workspace: &Arc<crate::workspace::Workspace>,
    machine: bool,
    render_tool: Option<Arc<dyn ChatTool>>,
) -> Vec<Arc<dyn ChatTool>> {
    let mut tools = super::files::file_tools(workspace);
    if machine {
        tools = tools
            .into_iter()
            .map(|tool| super::setup::guarded(tool, workspace))
            .collect();
        tools.extend(super::machine::machine_tools(workspace));
    }
    tools.extend(render_tool);
    tools.push(super::subagent::tool());
    tools
}

impl ChatTools {
    /// The list over `tools`, in that order.
    ///
    /// # Errors
    /// When a tool has no name, two tools share a name, or a schema is not a JSON object.
    pub fn new(tools: Vec<Arc<dyn ChatTool>>) -> Result<Self, InvalidTool> {
        if tools.is_empty() {
            return Ok(Self::default());
        }
        let mut schemas = Vec::with_capacity(tools.len());
        for (at, tool) in tools.iter().enumerate() {
            let name = tool.name();
            let invalid = |reason| InvalidTool {
                name: name.to_string(),
                reason,
            };
            if name.is_empty() {
                return Err(invalid("the name is empty"));
            }
            if tools[..at].iter().any(|earlier| earlier.name() == name) {
                return Err(invalid("another tool has the name"));
            }
            let schema = RawValue::from_string(tool.schema().to_string())
                .map_err(|_| invalid("the schema is not JSON"))?;
            if !schema.get().trim_start().starts_with('{') {
                return Err(invalid("the schema is not a JSON object"));
            }
            schemas.push(schema);
        }
        let listed: Vec<Listed> = tools
            .iter()
            .zip(&schemas)
            .map(|(tool, schema)| Listed {
                r#type: "function",
                function: Described {
                    name: tool.name(),
                    description: tool.description(),
                    parameters: schema,
                },
            })
            .collect();
        let listing = serde_json::value::to_raw_value(&listed).map_err(|_| InvalidTool {
            name: String::new(),
            reason: "the list cannot be written",
        })?;
        Ok(Self {
            tools,
            listing: Some(Arc::from(listing)),
        })
    }

    /// The list without the tool called `name`, in the same order.
    pub(super) fn without(&self, name: &str) -> Self {
        // Part of a list that was accepted is accepted.
        Self::new(
            self.tools
                .iter()
                .filter(|tool| tool.name() != name)
                .cloned()
                .collect(),
        )
        .unwrap_or_default()
    }

    /// The `tools` array of the request; `None` when there is no tool.
    pub(super) fn listing(&self) -> Option<&RawValue> {
        self.listing.as_deref()
    }

    /// The line for a call to `name` with `arguments`: the tool's own, or a fixed text when no tool has
    /// it. One line of at most [`MAX_LINE`] characters whatever the tool wrote.
    pub(super) fn line(&self, name: &str, arguments: &str) -> String {
        let line = self
            .find(name)
            .map_or_else(|| UNKNOWN_LINE.to_string(), |tool| tool.line(arguments));
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        let line = if line.is_empty() {
            name.to_string()
        } else {
            line
        };
        if line.chars().count() <= MAX_LINE {
            return line;
        }
        let mut cut: String = line.chars().take(MAX_LINE - 1).collect();
        cut.push('…');
        cut
    }

    /// The tool called `name`.
    pub(super) fn find(&self, name: &str) -> Option<Arc<dyn ChatTool>> {
        self.tools.iter().find(|tool| tool.name() == name).cloned()
    }
}

/// `text` as it goes to the model: at most [`MAX_RESULT`] bytes, cut at a character boundary,
/// with [`CUT_NOTE`] at the end when something was cut.
pub(super) fn limited(mut text: String) -> String {
    if text.len() <= MAX_RESULT {
        return text;
    }
    text.truncate(text.floor_char_boundary(MAX_RESULT - CUT_NOTE.len()));
    text.push_str(CUT_NOTE);
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Named(&'static str, &'static str);

    impl ChatTool for Named {
        fn name(&self) -> &str {
            self.0
        }

        fn description(&self) -> &'static str {
            "A tool."
        }

        fn schema(&self) -> &str {
            self.1
        }

        fn run(&self, _: &str) -> Result<String, String> {
            Ok(String::new())
        }
    }

    fn list(tools: &[(&'static str, &'static str)]) -> Result<ChatTools, InvalidTool> {
        ChatTools::new(
            tools
                .iter()
                .map(|&(name, schema)| Arc::new(Named(name, schema)) as Arc<dyn ChatTool>)
                .collect(),
        )
    }

    #[test]
    fn the_list_keeps_the_order_it_was_given() {
        let tools = list(&[("b", "{}"), ("a", "{}"), ("c", "{}")]).unwrap();
        let text = tools.listing().unwrap().get();
        let at = |name: &str| text.find(&format!("\"name\":\"{name}\"")).unwrap();
        assert!(at("b") < at("a") && at("a") < at("c"), "{text}");
        assert!(list(&[]).unwrap().listing().is_none());
    }

    #[test]
    fn a_list_that_a_provider_would_refuse_is_not_built() {
        for (tools, reason) in [
            (vec![("a", "{}"), ("a", "{}")], "another tool has the name"),
            (vec![("", "{}")], "the name is empty"),
            (vec![("a", "{not json")], "the schema is not JSON"),
            (vec![("a", "[]")], "the schema is not a JSON object"),
        ] {
            assert_eq!(list(&tools).err().unwrap().reason, reason);
        }
    }

    struct Reading;

    impl ChatTool for Reading {
        fn name(&self) -> &'static str {
            "read"
        }

        fn description(&self) -> &'static str {
            "A tool."
        }

        fn schema(&self) -> &'static str {
            "{}"
        }

        fn line(&self, arguments: &str) -> String {
            serde_json::from_str::<serde_json::Value>(arguments)
                .ok()
                .and_then(|v| v["path"].as_str().map(|p| format!("Read {p}")))
                .unwrap_or_else(|| self.name().to_string())
        }

        fn run(&self, _: &str) -> Result<String, String> {
            Ok(String::new())
        }
    }

    #[test]
    fn a_line_is_one_short_line_and_falls_back_to_the_name() {
        let tools = ChatTools::new(vec![Arc::new(Reading) as Arc<dyn ChatTool>]).unwrap();
        assert_eq!(
            tools.line("read", r#"{"path":"notes.txt"}"#),
            "Read notes.txt"
        );
        assert_eq!(tools.line("read", "{not json"), "read");
        assert_eq!(tools.line("nothing", "{}"), UNKNOWN_LINE);
        assert_eq!(tools.line("read", r#"{"path":"a\nb\tc"}"#), "Read a b c");
        let long = tools.line("read", &format!(r#"{{"path":"{}"}}"#, "x".repeat(500)));
        assert_eq!(long.chars().count(), MAX_LINE);
        assert!(long.ends_with('…'));
        // A tool that gives no line of its own shows its name, whatever the arguments hold.
        let plain = list(&[("plain", "{}")]).unwrap();
        assert_eq!(plain.line("plain", r#"{"q":"secret"}"#), "plain");
    }

    #[test]
    fn a_short_result_is_kept_and_a_long_one_is_cut_at_a_character() {
        assert_eq!(limited("ok".into()), "ok");
        let exact = "a".repeat(MAX_RESULT);
        assert_eq!(limited(exact.clone()), exact);
        // Two-byte characters: the limit for the kept text falls inside one.
        let cut = limited(format!("a{}", "é".repeat(MAX_RESULT)));
        let kept = cut.strip_suffix(CUT_NOTE).unwrap();
        assert_eq!(kept.len(), MAX_RESULT - CUT_NOTE.len() - 1);
        assert!(kept.starts_with('a') && kept.ends_with('é'));
        assert!(cut.len() <= MAX_RESULT);
    }
}
