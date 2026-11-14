//! Readers of the session logs of coding tools (Claude Code, Codex, ...).
//!
//! A [`Tool`] knows where its session files live and how to turn one line of such a
//! file into a [`Record`]. The crate does no I/O of its own besides [`read_new_lines`]
//! and depends on neither the agent nor an async runtime.

mod claude_code;
mod codex;
mod lines;
mod parsed;
mod record;
mod text;
mod tool;

pub use claude_code::ClaudeCode;
pub use codex::Codex;
pub use lines::read_new_lines;
pub use parsed::Parsed;
pub use record::{Mark, Record, Role, Turn, compare_times, minute_of};
pub use tool::{FileState, Registry, Tool, tools};
