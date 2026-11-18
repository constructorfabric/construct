//! What the findings say, as statements of the person, one to a line, for the facts route.

use std::fmt::Write as _;

use serde_json::Value;

use crate::activity::findings::{list, text};

/// The longest piece of a finding that goes into a fact, in characters.
const MAX_PIECE: usize = 300;

/// A string member of `object`, as the assignment of sessions reads it and cut.
fn piece(object: &Value, member: &str) -> String {
    cut(&text(object, member))
}

fn pieces(object: &Value, member: &str) -> Vec<String> {
    list(object, member)
        .iter()
        .map(|piece| cut(piece))
        .collect()
}

fn cut(piece: &str) -> String {
    piece.chars().take(MAX_PIECE).collect()
}

/// What the findings say, as statements of the person, one to a line, for the facts route.
pub(super) fn lines(findings: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    for tracker in findings
        .get("trackers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = piece(tracker, "name");
        if name.is_empty() {
            continue;
        }
        let kind = piece(tracker, "kind");
        lines.push(if kind.is_empty() {
            format!("I work with the task tracker {name}.")
        } else {
            format!("I work with the task tracker {name} ({kind}).")
        });
    }
    for mapping in findings
        .get("mappings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let (tracker, project) = (piece(mapping, "tracker"), piece(mapping, "project"));
        let repository = piece(mapping, "repository");
        let folders = pieces(mapping, "folders");
        if repository.is_empty() && project.is_empty() {
            continue;
        }
        let mut line = if project.is_empty() {
            format!("I work on the repository {repository}, which belongs to no tracker project.")
        } else if repository.is_empty() {
            format!("I work on the project {project} of the task tracker {tracker}.")
        } else {
            format!(
                "I work on the repository {repository}, which is the project {project} of the task tracker {tracker}."
            )
        };
        if !folders.is_empty() {
            let _ = write!(line, " It is in my folders {}.", folders.join(", "));
        }
        if mapping.get("important").and_then(Value::as_bool) == Some(true) {
            line.push_str(" It is one of the projects that matter most to me.");
        }
        lines.push(line);
    }
    for grouping in findings
        .get("groupings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let (name, projects) = (piece(grouping, "name"), pieces(grouping, "projects"));
        if !name.is_empty() && !projects.is_empty() {
            lines.push(format!(
                "I group my projects {} together as {name}.",
                projects.join(", ")
            ));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_findings_become_one_statement_to_a_line() {
        let findings = json!({
            "trackers": [{ "name": "alpha", "kind": "issue tracker" }, { "name": "" }],
            "mappings": [
                { "tracker": "alpha", "project": "PLAT", "repository": "git@h:t/shop.git",
                  "folders": ["shop"], "important": true },
                { "repository": "git@h:t/lab.git", "folders": [] },
            ],
            "groupings": [{ "name": "work", "projects": ["PLAT", "BLOG"] }],
        });
        let lines = lines(&findings);
        assert_eq!(lines.len(), 4);
        assert_eq!(
            lines[0],
            "I work with the task tracker alpha (issue tracker)."
        );
        assert!(lines[1].contains("project PLAT of the task tracker alpha"));
        assert!(lines[1].ends_with("matter most to me."));
        assert!(lines[2].contains("belongs to no tracker project"));
        assert_eq!(lines[3], "I group my projects PLAT, BLOG together as work.");
    }
}
