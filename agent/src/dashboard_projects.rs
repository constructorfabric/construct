//! Hiding and renaming projects in the dashboard document, and the rows of a project table
//! once they are applied.
//!
//! A project is named by its folder, as the store keeps it. The document lists the folders the
//! page hides (`hidden_projects`) and the name it shows for a folder (`project_names`). The
//! edits here change those two members of the payload and nothing else.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

/// A change to the projects of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Leave the project out of the page.
    Hide(String),
    /// Bring a hidden project back.
    Show(String),
    /// Show the project under this name.
    Rename { project: String, name: String },
}

impl Edit {
    /// Reads the argument of a socket command: a JSON object with `project`, and `name` for a
    /// rename. Any other member is refused.
    ///
    /// # Errors
    /// The text is not that object, or a member is missing or empty.
    pub fn parse(command: &str, argument: &str) -> Result<Self, &'static str> {
        let object: Value =
            serde_json::from_str(argument).map_err(|_| "the request is not a JSON object")?;
        let object = object
            .as_object()
            .ok_or("the request is not a JSON object")?;
        let wanted: &[&str] = if command == "dashboard-rename" {
            &["project", "name"]
        } else {
            &["project"]
        };
        if object.keys().any(|key| !wanted.contains(&key.as_str())) {
            return Err("the request has a member this command does not take");
        }
        let text = |member: &str| match object.get(member) {
            Some(Value::String(text)) if !text.trim().is_empty() => Ok(text.clone()),
            Some(Value::String(_)) => Err("a project and a name cannot be empty"),
            _ => Err("the request needs a project, and a name for a rename"),
        };
        let project = text("project")?;
        match command {
            "dashboard-hide" => Ok(Self::Hide(project)),
            "dashboard-show" => Ok(Self::Show(project)),
            "dashboard-rename" => Ok(Self::Rename {
                project,
                name: text("name")?,
            }),
            _ => Err("unknown dashboard command"),
        }
    }

    /// Applies the edit to the payload of `document` and leaves every other member as it was.
    ///
    /// The document must have passed its type, which gives it the list and the object the edit
    /// changes; one that has not is left as it is. Hiding a hidden project, showing one that is
    /// not hidden and a rename to the name it has change nothing.
    pub fn apply(&self, document: &mut Value) {
        let Some(payload) = document.get_mut("payload") else {
            return;
        };
        match self {
            Self::Hide(project) | Self::Show(project) => {
                if let Some(hidden) = payload
                    .get_mut("hidden_projects")
                    .and_then(Value::as_array_mut)
                {
                    let listed = hidden.iter().any(|entry| entry.as_str() == Some(project));
                    match (self, listed) {
                        (Self::Hide(_), false) => hidden.push(json!(project)),
                        (Self::Show(_), true) => hidden.retain(|e| e.as_str() != Some(project)),
                        _ => {}
                    }
                }
            }
            Self::Rename { project, name } => {
                if let Some(names) = payload
                    .get_mut("project_names")
                    .and_then(Value::as_object_mut)
                {
                    names.insert(project.clone(), json!(name));
                }
            }
        }
    }
}

/// One row of a project table: the name shown and the time of every project under it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub name: String,
    /// In the unit the caller gave.
    pub parts: u64,
    /// The share of the total of the rows shown, from 0 to 1; 0 when the total is 0.
    pub share: f64,
}

/// The rows of a project table once the hidden projects are out and the names are applied.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    /// The longest first, ties by name.
    pub rows: Vec<Row>,
    /// The time of the rows shown: what the hidden projects had is not in it.
    pub total_parts: u64,
    /// How many projects the document hides.
    pub hidden_count: usize,
}

/// Applies the document's `hidden_projects` and `project_names` to `projects`, the time of each
/// project by its folder. `labels` gives the name a project has when the document names it
/// none; a folder with no label is shown as the folder. Two projects with one name are one row.
#[must_use]
pub fn view(
    projects: &[(String, u64)],
    labels: &BTreeMap<String, String>,
    document: &Value,
) -> View {
    let hidden: BTreeSet<&str> = document
        .pointer("/payload/hidden_projects")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let names = document.pointer("/payload/project_names");
    let mut rows: BTreeMap<String, u64> = BTreeMap::new();
    for (project, parts) in projects {
        if hidden.contains(project.as_str()) {
            continue;
        }
        let name = names
            .and_then(|names| names.get(project))
            .and_then(Value::as_str)
            .or_else(|| labels.get(project).map(String::as_str))
            .unwrap_or(project);
        *rows.entry(name.to_owned()).or_default() += parts;
    }
    let total_parts = rows.values().sum();
    let mut rows: Vec<Row> = rows
        .into_iter()
        .map(|(name, parts)| Row {
            name,
            parts,
            #[allow(clippy::cast_precision_loss)] // a total never nears 2^52 parts
            share: if total_parts == 0 {
                0.0
            } else {
                parts as f64 / total_parts as f64
            },
        })
        .collect();
    rows.sort_by(|a, b| b.parts.cmp(&a.parts).then_with(|| a.name.cmp(&b.name)));
    View {
        rows,
        total_parts,
        hidden_count: hidden.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_that_lacks_what_the_edit_changes_is_left_as_it_is() {
        for original in [
            json!({ "title": "x" }),
            json!({ "payload": 5 }),
            json!({ "payload": { "hidden_projects": null } }),
            json!({ "payload": {} }),
        ] {
            for edit in [
                Edit::Hide("/a".to_owned()),
                Edit::Show("/a".to_owned()),
                Edit::Rename {
                    project: "/a".to_owned(),
                    name: "A".to_owned(),
                },
            ] {
                let mut document = original.clone();
                edit.apply(&mut document);
                assert_eq!(document, original, "{edit:?}");
            }
        }
    }
}
