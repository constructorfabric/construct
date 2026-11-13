//! Construct agent skeleton: an own program that keeps running after the app
//! window is closed. It answers a `status` call on a user-only Unix socket.

pub mod autostart;
pub mod ipc;
