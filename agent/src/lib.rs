//! Construct agent: an own program that keeps running after the app
//! window is closed. It answers a `status` call on a user-only Unix socket.

#[cfg(unix)]
pub mod activity;
pub mod autostart;
#[cfg(unix)]
pub mod chat;
#[cfg(unix)]
pub mod dashboard;
#[cfg(unix)]
pub mod dashboard_details;
#[cfg(unix)]
pub mod dashboard_document;
#[cfg(unix)]
pub mod dashboard_numbers;
pub mod dashboard_projects;
#[cfg(unix)]
pub mod database;
pub mod follow;
#[cfg(unix)]
pub mod host;
pub mod ipc;
#[cfg(unix)]
pub mod manage;
#[cfg(unix)]
pub mod model;
#[cfg(unix)]
pub mod model_test;
#[cfg(unix)]
pub mod pgvector;
pub mod records;
#[cfg(unix)]
pub mod session_files;
#[cfg(unix)]
pub mod settings;
mod sync;
#[cfg(unix)]
pub mod workspace;
