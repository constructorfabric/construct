//! Readers of the session logs of coding tools (Claude Code, Codex, ...).
//!
//! A [`Tool`] knows where its session files live and how to turn one line of such a
//! file into a [`Record`]. The crate does no I/O of its own besides [`read_new_lines`]
//! and depends on neither the agent nor an async runtime.

mod lines;
mod record;
mod tool;

pub use lines::read_new_lines;
pub use record::{Mark, Record, Role, Turn, compare_times, is_utc_time, minute_of};
pub use tool::{FileState, Registry, Tool, tools};
