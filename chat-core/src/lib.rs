//! The chat loop of Construct, with the host behind traits.
//!
//! A host (the desktop agent, an app on a phone) gives the crate four things:
//!
//! - the connection to the provider: the model's address, name and key, read for every call
//!   ([`model::ModelSource`]);
//! - the chat records: where the conversations are kept ([`records::Records`]);
//! - the facts of the person ([`recall::Facts`]);
//! - the tools: a tool file, read once at the host's start, with the name, description and
//!   argument schema of each tool and their order, and a handler for each name
//!   ([`tool::ToolConfig`], [`tool::ToolHandler`]).
//!
//! The crate also holds the model's settings type with the check of its address and the Test call
//! of the Settings page ([`model_test`]). It has no agent, no gateway, no database and nothing
//! specific to one operating system.

pub mod chat;
pub mod model;
pub mod model_test;
pub mod recall;
pub mod records;
mod sse;
pub mod subagent;
mod sync;
pub mod tool;
mod usage;
pub mod wire;

pub use chat::{Chat, Deadlines, Failure, Piece, Run, ToolCount};
