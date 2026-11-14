//! Construct agent: an own program that keeps running after the app
//! window is closed. It answers a `status` call on a user-only Unix socket and runs its own
//! PostgreSQL with pgvector, which `status` reports as `database`.

pub mod autostart;
#[cfg(unix)]
pub mod database;
pub mod ipc;
#[cfg(unix)]
pub mod pgvector;
pub mod records;
