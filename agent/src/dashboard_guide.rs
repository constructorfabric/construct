//! The guide files for the chat: `dashboard.schema.json` and `dashboard.md`, two ordinary text
//! files in the workspace that tell the model what the dashboard document is and how to edit it.
//!
//! [`write`] replaces both at every start of the agent, so they always match this build and
//! whatever the model changed in them is undone. The chat's system instruction does not mention
//! them; the description of the `render_dashboard` tool names them.

use crate::dashboard::document_schema;
use crate::workspace::{Error, Workspace};

/// The file with the schema of the dashboard type, relative to the workspace.
pub const SCHEMA_FILE: &str = "dashboard.schema.json";
/// The file with the guide for the model, relative to the workspace.
pub const GUIDE_FILE: &str = "dashboard.md";
/// The guide: plain English, with JSON examples that are whole dashboard documents.
pub const GUIDE: &str = include_str!("dashboard_guide.md");

/// Writes both files into the workspace, replacing them when they are there.
///
/// # Errors
/// The workspace refuses to write one of the files.
pub fn write(workspace: &Workspace) -> Result<(), Error> {
    let mut schema =
        serde_json::to_string_pretty(&document_schema()).map_err(|_| Error::NotText)?;
    schema.push('\n');
    workspace.write(SCHEMA_FILE, &schema)?;
    workspace.write(GUIDE_FILE, GUIDE)
}
