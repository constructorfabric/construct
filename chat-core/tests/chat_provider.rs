//! The loop's request to the provider: a redirect is not followed, so the key goes only to the
//! address the settings name.
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::sync::Arc;
use std::time::Duration;

use chat_core::{Chat, Deadlines};

mod common;

use common::{Fake, Fixed, Memory, Reply, model_at};

#[test]
fn a_redirect_from_the_provider_is_an_error_and_is_not_followed() {
    let elsewhere = Fake::start(vec![Reply::Status(200, "{}")]);
    let provider = Fake::start(vec![Reply::Redirect(format!(
        "http://127.0.0.1:{}/v1/chat/completions",
        elsewhere.addr.port()
    ))]);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let chat = Chat::new(
        Arc::new(Fixed(model_at(provider.addr))),
        Box::new(Memory::default()),
        rt.handle().clone(),
        Deadlines {
            connect: Duration::from_secs(5),
            first_word: Duration::from_secs(10),
            silence: Duration::from_secs(10),
        },
    )
    .unwrap();
    let mut lines = Vec::new();
    chat.send(r#"{"message":"hi"}"#, &mut |line| {
        lines.push(line.to_string());
        true
    });
    assert_eq!(
        lines.last().unwrap(),
        r#"{"event":"error","reason":"error 302"}"#
    );
    assert_eq!(provider.bodies.lock().unwrap().len(), 1);
    assert!(elsewhere.bodies.lock().unwrap().is_empty());
}
