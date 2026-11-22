//! The model Test call, `settings-test`: it lives in the `chat-core` crate, which the phone app
//! shares; the agent serves it over its socket (see `ipc`).

pub use chat_core::model_test::*;
