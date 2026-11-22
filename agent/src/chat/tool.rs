//! The tools of the desktop chat: the lists the host offers, and the tool file they come from.
//!
//! The name, the description, the argument schema and the order of every tool are in
//! `tools/desktop.json`, bundled with the app and read once at the start of the host (see
//! [`desktop_config`]); a tool here only has a handler. The trait and the list type are the
//! `chat-core` crate's.

use std::sync::{Arc, OnceLock};

pub use chat_core::tool::*;

use super::subagent;
use crate::workspace::Workspace;

/// The tool file of the desktop, as bundled.
const DESKTOP_TOOLS: &str = include_str!("../../../tools/desktop.json");

/// The tool file, parsed once.
fn parsed() -> &'static Result<ToolConfig, ConfigError> {
    static CONFIG: OnceLock<Result<ToolConfig, ConfigError>> = OnceLock::new();
    CONFIG.get_or_init(|| ToolConfig::parse(DESKTOP_TOOLS))
}

/// The tool file of the desktop. The host asks for it once at its start, so that a file that does
/// not hold is a plain error then and not a chat without tools later.
///
/// # Errors
/// When the bundled file is not a list of tools.
pub fn desktop_config() -> Result<&'static ToolConfig, &'static ConfigError> {
    parsed().as_ref()
}

/// The description of the tool `name`, from the tool file. A tool of the code that the file does not
/// list has none: the host's start refuses such a list (see [`ChatTools::configured`]).
pub(super) fn description(name: &str) -> &'static str {
    desktop_config()
        .ok()
        .and_then(|config| config.text(name))
        .map_or("", |(description, _)| description)
}

/// The argument schema of the tool `name`, from the tool file, as written there.
pub(super) fn schema(name: &str) -> &'static str {
    desktop_config()
        .ok()
        .and_then(|config| config.text(name))
        .map_or("{}", |(_, schema)| schema)
}

/// The tools of a chat over `workspace`, in the order the model is given them: the file tools, the
/// tool that renders the dashboard when there is one, and the sub-agent tool last.
#[must_use]
pub fn standard_tools(
    workspace: &Arc<Workspace>,
    render_tool: Option<Arc<dyn ChatTool>>,
) -> Vec<Arc<dyn ChatTool>> {
    listed(workspace, false, render_tool)
}

/// The tools of the setup session over `workspace`: those of [`standard_tools`] with the tools that
/// read the user's machine after the file tools, and the file tools unable to change the folder
/// list those tools take their folders from.
#[must_use]
pub fn setup_tools(
    workspace: &Arc<Workspace>,
    render_tool: Option<Arc<dyn ChatTool>>,
) -> Vec<Arc<dyn ChatTool>> {
    listed(workspace, true, render_tool)
}

fn listed(
    workspace: &Arc<Workspace>,
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
    tools.push(subagent::tool());
    tools
}

/// The names of the tools of [`standard_tools`], in order. The dashboard tool is left out when
/// `render` is false.
fn standard_names(render: bool) -> Vec<&'static str> {
    let mut names = vec!["Read", "Write", "Edit", "Glob"];
    if render {
        names.push(super::dashboard_view::NAME);
    }
    names.push(subagent::NAME);
    names
}

/// The list of the chat: the entries of the tool file that the chat offers, run by `tools`.
///
/// # Errors
/// When the tool file lacks a tool the chat offers, or lists one that `tools` has no handler for.
pub fn chat_list(tools: Vec<Arc<dyn ChatTool>>, render: bool) -> Result<ChatTools, String> {
    let config = desktop_config().map_err(ToString::to_string)?;
    list_from(config, &standard_names(render), tools)
}

/// The list of the setup session: the entries of the tool file, run by `tools`. The dashboard tool
/// is left out when `render` is false.
///
/// # Errors
/// When the tool file lists a tool that `tools` has no handler for, or lists none.
pub fn setup_list(tools: Vec<Arc<dyn ChatTool>>, render: bool) -> Result<ChatTools, String> {
    let config = desktop_config().map_err(ToString::to_string)?;
    let names: Vec<&str> = config
        .names()
        .into_iter()
        .filter(|name| render || *name != super::dashboard_view::NAME)
        .collect();
    list_from(config, &names, tools)
}

/// The entries of `config` called one of `names`, run by `tools`. A host that must offer a tool
/// does not start with a file that lacks it, and a list with no tool is no list.
fn list_from(
    config: &ToolConfig,
    names: &[&str],
    tools: Vec<Arc<dyn ChatTool>>,
) -> Result<ChatTools, String> {
    let have = config.names();
    if let Some(missing) = names.iter().find(|name| !have.contains(name)) {
        return Err(format!("the tool file lacks the tool `{missing}`"));
    }
    if names.is_empty() {
        return Err("the tool file lists no tool".to_string());
    }
    let handlers: Vec<_> = tools.into_iter().map(handler).collect();
    ChatTools::configured(&config.only(names), &handlers).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(&'static str);

    impl ChatTool for Fake {
        fn name(&self) -> &str {
            self.0
        }

        fn description(&self) -> &'static str {
            ""
        }

        fn schema(&self) -> &'static str {
            "{}"
        }

        fn run(&self, _: &str) -> Result<String, String> {
            Ok(String::new())
        }
    }

    #[test]
    fn a_file_that_lacks_a_tool_the_host_offers_or_lists_none_is_refused() {
        let file = ToolConfig::parse(r#"[{"name":"Read","description":"d","schema":{}}]"#).unwrap();
        let tools = || vec![Arc::new(Fake("Read")) as Arc<dyn ChatTool>];
        assert!(list_from(&file, &["Read"], tools()).is_ok());
        let error = list_from(&file, &["Read", "Glob"], tools()).err().unwrap();
        assert_eq!(error, "the tool file lacks the tool `Glob`");
        let empty = ToolConfig::parse("[]").unwrap();
        assert_eq!(
            list_from(&empty, &[], Vec::new()).err().unwrap(),
            "the tool file lists no tool"
        );
        // A tool the file lists and the host cannot run is refused too.
        let error = list_from(&file, &["Read"], Vec::new()).err().unwrap();
        assert!(error.contains("no handler"), "{error}");
    }
}
