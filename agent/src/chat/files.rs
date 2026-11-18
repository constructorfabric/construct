//! The file tools of the chat: `Read`, `Write`, `Edit` and `Glob` over the workspace folder.
//!
//! Names and argument names are those of Claude Code, and the schemas are written by hand. Results
//! are plain text; an error is the plain text of the workspace's error. The lines the window shows
//! name the file or the pattern, never the content, and nothing here is logged.

use std::fmt::Write as _;
use std::sync::Arc;

use serde::Deserialize;

use super::tool::{ChatTool, MAX_RESULT};
use crate::workspace::Workspace;

/// The result when the arguments are not what the schema says.
pub(super) const BAD_ARGUMENTS: &str = "the arguments are not valid for this tool";
/// The result of a `Read` of a file with no line.
const EMPTY_FILE: &str = "The file is empty.";
/// The result of a `Read` that starts after the last line.
const PAST_END: &str = "The file has no line at this offset.";
/// The result of a `Glob` that finds nothing.
const NO_FILES: &str = "No files match.";
/// Bytes kept free at the end of a `Read` for the note of a cut.
const NOTE_ROOM: usize = 160;

/// The four tools over `workspace`, in the order the model is given them.
#[must_use]
pub fn file_tools(workspace: &Arc<Workspace>) -> Vec<Arc<dyn ChatTool>> {
    vec![
        Arc::new(Read(Arc::clone(workspace))),
        Arc::new(Write(Arc::clone(workspace))),
        Arc::new(Edit(Arc::clone(workspace))),
        Arc::new(Glob(Arc::clone(workspace))),
    ]
}

pub(super) fn parse<'a, T: Deserialize<'a>>(arguments: &'a str) -> Result<T, String> {
    serde_json::from_str(arguments).map_err(|_| BAD_ARGUMENTS.to_string())
}

/// The status line `verb` plus the text of `field` in the arguments, or `name` when there is none.
pub(super) fn line(arguments: &str, field: &str, verb: &str, name: &str) -> String {
    serde_json::from_str::<serde_json::Value>(arguments)
        .ok()
        .and_then(|v| v[field].as_str().map(|text| format!("{verb} {text}")))
        .unwrap_or_else(|| name.to_string())
}

struct Read(Arc<Workspace>);

#[derive(Deserialize)]
struct ReadArguments {
    file_path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

impl ChatTool for Read {
    fn name(&self) -> &'static str {
        "Read"
    }

    fn description(&self) -> &'static str {
        "Reads a text file from the workspace folder. The result has one line for each line of the \
         file, as a line number, a tab and the text. Use `offset` (the number of the first line) and \
         `limit` (the most lines) to read a part of a long file. A long result is cut, and its last \
         line says which offset to continue with."
    }

    fn schema(&self) -> &'static str {
        r#"{"type":"object","properties":{"file_path":{"type":"string","description":"The path of the file, relative to the workspace folder."},"offset":{"type":"integer","description":"The number of the first line to read. The first line of the file is 1."},"limit":{"type":"integer","description":"The most lines to read."}},"required":["file_path"]}"#
    }

    fn line(&self, arguments: &str) -> String {
        line(arguments, "file_path", "Read", self.name())
    }

    fn run(&self, arguments: &str) -> Result<String, String> {
        let args: ReadArguments = parse(arguments)?;
        let lines = self
            .0
            .read(&args.file_path, args.offset, args.limit)
            .map_err(|e| e.to_string())?;
        let first = args.offset.unwrap_or(1).max(1);
        Ok(numbered(&lines, first, args.offset.is_some_and(|n| n > 1)))
    }
}

/// `lines` as numbered text, the first being line `first`, cut so that the text, note included, is
/// at most [`MAX_RESULT`] bytes. A cut ends with a note naming the offset to go on with.
/// `shifted` is whether the read started after the first line.
pub(super) fn numbered(lines: &[String], first: usize, shifted: bool) -> String {
    if lines.is_empty() {
        return if shifted { PAST_END } else { EMPTY_FILE }.to_string();
    }
    let budget = MAX_RESULT - NOTE_ROOM;
    let mut text = String::new();
    for (at, content) in lines.iter().enumerate() {
        let number = first + at;
        let row = format!("{number:>6}\t{content}\n");
        if text.len() + row.len() > budget {
            if at == 0 {
                // One line is longer than the whole result: show its start, go on with the next.
                let mut head = format!("{number:>6}\t");
                head.push_str(&content[..content.floor_char_boundary(budget - head.len())]);
                text = head;
                text.push('\n');
                let _ = write!(
                    text,
                    "(Line {number} is too long and was cut. Continue with offset {}.)",
                    number + 1
                );
            } else {
                let _ = write!(
                    text,
                    "(The result was cut. Continue with offset {number} to read the rest.)"
                );
            }
            return text;
        }
        text.push_str(&row);
    }
    text.pop();
    text
}

struct Write(Arc<Workspace>);

#[derive(Deserialize)]
struct WriteArguments {
    file_path: String,
    content: String,
}

impl ChatTool for Write {
    fn name(&self) -> &'static str {
        "Write"
    }

    fn description(&self) -> &'static str {
        "Writes a text file in the workspace folder. It replaces the file when it exists and creates \
         it, with its folders, when it does not. Give the text of the file as it is, with no line \
         numbers."
    }

    fn schema(&self) -> &'static str {
        r#"{"type":"object","properties":{"file_path":{"type":"string","description":"The path of the file, relative to the workspace folder."},"content":{"type":"string","description":"The whole text of the file."}},"required":["file_path","content"]}"#
    }

    fn line(&self, arguments: &str) -> String {
        line(arguments, "file_path", "Wrote", self.name())
    }

    fn run(&self, arguments: &str) -> Result<String, String> {
        let args: WriteArguments = parse(arguments)?;
        self.0
            .write(&args.file_path, &args.content)
            .map_err(|e| e.to_string())?;
        Ok(format!(
            "Wrote {} bytes to {}",
            args.content.len(),
            args.file_path
        ))
    }
}

struct Edit(Arc<Workspace>);

#[derive(Deserialize)]
struct EditArguments {
    file_path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

impl ChatTool for Edit {
    fn name(&self) -> &'static str {
        "Edit"
    }

    fn description(&self) -> &'static str {
        "Replaces text in a file of the workspace folder. `old_string` must occur once in the file, \
         unless `replace_all` is true, which replaces every occurrence. Give both strings as they \
         are in the file, with no line numbers."
    }

    fn schema(&self) -> &'static str {
        r#"{"type":"object","properties":{"file_path":{"type":"string","description":"The path of the file, relative to the workspace folder."},"old_string":{"type":"string","description":"The text to replace."},"new_string":{"type":"string","description":"The text to put in its place. It must differ from old_string."},"replace_all":{"type":"boolean","description":"Replace every occurrence of old_string. The default is false."}},"required":["file_path","old_string","new_string"]}"#
    }

    fn line(&self, arguments: &str) -> String {
        line(arguments, "file_path", "Changed", self.name())
    }

    fn run(&self, arguments: &str) -> Result<String, String> {
        let args: EditArguments = parse(arguments)?;
        self.0
            .edit(
                &args.file_path,
                &args.old_string,
                &args.new_string,
                args.replace_all,
            )
            .map_err(|e| e.to_string())?;
        Ok(format!("Changed {}", args.file_path))
    }
}

struct Glob(Arc<Workspace>);

#[derive(Deserialize)]
struct GlobArguments {
    pattern: String,
    path: Option<String>,
}

impl ChatTool for Glob {
    fn name(&self) -> &'static str {
        "Glob"
    }

    fn description(&self) -> &'static str {
        "Finds files of the workspace folder by a glob pattern such as `**/*.md`. The result is one \
         path for each line, sorted."
    }

    fn schema(&self) -> &'static str {
        r#"{"type":"object","properties":{"pattern":{"type":"string","description":"The glob pattern, for example **/*.txt."},"path":{"type":"string","description":"The folder to search in, relative to the workspace folder. The default is the whole workspace."}},"required":["pattern"]}"#
    }

    fn line(&self, arguments: &str) -> String {
        line(arguments, "pattern", "Searched", self.name())
    }

    fn run(&self, arguments: &str) -> Result<String, String> {
        let args: GlobArguments = parse(arguments)?;
        let found = self
            .0
            .glob(&args.pattern, args.path.as_deref().unwrap_or(""))
            .map_err(|e| e.to_string())?;
        if found.is_empty() {
            return Ok(NO_FILES.to_string());
        }
        Ok(found.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(n: usize) -> Vec<String> {
        (1..=n).map(|i| format!("line {i}")).collect()
    }

    #[test]
    fn lines_are_numbered_like_cat_dash_n() {
        let text = numbered(&lines(2), 1, false);
        assert_eq!(text, "     1\tline 1\n     2\tline 2");
        assert_eq!(numbered(&lines(1), 41, true), "    41\tline 1");
    }

    #[test]
    fn an_empty_read_says_why() {
        assert_eq!(numbered(&[], 1, false), EMPTY_FILE);
        assert_eq!(numbered(&[], 9, true), PAST_END);
    }

    #[test]
    fn a_long_read_is_cut_with_the_next_offset() {
        let long = vec!["x".repeat(1000); 200];
        let text = numbered(&long, 1, false);
        assert!(text.len() <= MAX_RESULT);
        let note = text.lines().last().unwrap();
        let shown = text.lines().count() - 1;
        assert!(note.contains(&format!("offset {}", shown + 1)), "{note}");
    }

    #[test]
    fn one_line_over_the_limit_is_cut_and_the_next_line_is_offered() {
        let long = vec!["é".repeat(MAX_RESULT), "next".into()];
        let text = numbered(&long, 5, false);
        assert!(text.len() <= MAX_RESULT);
        assert!(
            text.ends_with("Continue with offset 6.)"),
            "{}",
            &text[text.len() - 60..]
        );
    }
}
