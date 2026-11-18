//! Fixtures of the tests that read the real Claude Code and Codex session files over a temporary
//! home.
#![allow(clippy::unwrap_used, dead_code)]

use std::io;
use std::path::{Path, PathBuf};

use connectors::{FileState, Listing, Parsed, Tool};

/// A real tool that looks for its files under a directory of the test's home instead of
/// the one its environment variable names; everything else is the tool's own.
pub struct Under<T>(pub T, pub &'static str);

impl<T: Tool> Tool for Under<T> {
    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn root(&self, home: &Path) -> PathBuf {
        home.join(self.1)
    }

    fn find_files(&self, root: &Path) -> io::Result<Listing> {
        self.0.find_files(root)
    }

    fn open_file(&self, path: &Path) -> FileState {
        self.0.open_file(path)
    }

    fn parse_line(&self, state: &mut FileState, line: &str) -> Parsed {
        self.0.parse_line(state, line)
    }
}

/// A user line of a Claude Code session file.
pub fn claude_user(text: &str) -> String {
    format!(
        r#"{{"type":"user","timestamp":"2026-11-11T10:15:30.123Z","cwd":"/w","sessionId":"sess-1","message":{{"role":"user","content":"{text}"}}}}"#
    )
}

/// A line of a Codex session file: `kind` is its `type`, `payload` its JSON payload.
pub fn codex_item(kind: &str, payload: &str) -> String {
    format!(r#"{{"timestamp":"2026-11-11T10:15:30.123Z","type":"{kind}","payload":{payload}}}"#)
}

/// The first line of a Codex session file.
pub fn codex_meta(id: &str) -> String {
    codex_item("session_meta", &format!(r#"{{"id":"{id}","cwd":"/w"}}"#))
}

/// A user message line of a Codex session file.
pub fn codex_user(text: &str) -> String {
    codex_item(
        "response_item",
        &format!(
            r#"{{"type":"message","role":"user","content":[{{"type":"input_text","text":"{text}"}}]}}"#
        ),
    )
}
