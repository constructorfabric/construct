//! The Test call of the Settings page against a fake provider on a local port: one test for each
//! reason a check can fail for, and the one that works.
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::time::Duration;

use chat_core::model::Model;
use chat_core::model_test::{checks, run_checks};
use serde_json::{Value, json};

mod common;

use common::{Fake, Reply, model_at};

const KEY: &str = "sk-test-key-4f9a";

/// The lines of an answer to the Test call with `model` stored and `deadline` for each check.
fn test_call(model: &Model, deadline: Duration) -> Vec<Value> {
    let mut lines = Vec::new();
    run_checks("", model, deadline, &checks(), &mut |line| {
        lines.push(serde_json::from_str(line).unwrap());
        true
    });
    lines
}

fn last(lines: &[Value]) -> &Value {
    assert_eq!(lines.len(), 1, "{lines:?}");
    &lines[0]
}

/// The first check failed with `result`, and the check after it did not run.
fn failed_with(answer: &Value, result: &str) {
    assert_eq!(answer["result"], result, "{answer}");
    assert_eq!(
        answer["checks"],
        json!([
            { "name": "answers", "result": result },
            { "name": "uses tools", "result": "skipped" },
        ]),
        "{answer}"
    );
    assert!(!answer.to_string().contains(KEY), "{answer}");
}

#[test]
fn a_provider_that_answers_works() {
    let completion = r#"{"choices":[{"message":{"content":"pong"}}]}"#;
    let tool_call = r#"{"choices":[{"message":{"content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"add","arguments":"{\"a\":2,\"b\":3}"}}]}}]}"#;
    let sum = r#"{"choices":[{"message":{"content":"The sum is 5."}}]}"#;
    let provider = Fake::start(vec![
        Reply::Status(200, completion),
        Reply::Status(200, tool_call),
        Reply::Status(200, sum),
    ]);
    let lines = test_call(&model_at(provider.addr), Duration::from_secs(10));
    let answer = last(&lines);
    assert_eq!(answer["result"], "works", "{answer}");
    assert_eq!(answer["text"], "pong");
    assert_eq!(provider.bodies.lock().unwrap().len(), 3);
}

#[test]
fn a_key_the_provider_refuses_is_key_refused() {
    let provider = Fake::start(vec![Reply::Status(
        401,
        r#"{"error":"bad key sk-test-key-4f9a"}"#,
    )]);
    let lines = test_call(&model_at(provider.addr), Duration::from_secs(10));
    failed_with(last(&lines), "key refused");
}

#[test]
fn a_model_the_provider_does_not_know_is_model_not_found() {
    let provider = Fake::start(vec![Reply::Status(404, "{}")]);
    let lines = test_call(&model_at(provider.addr), Duration::from_secs(10));
    failed_with(last(&lines), "model not found");
}

#[test]
fn a_provider_that_cannot_be_reached_is_unreachable() {
    let lines = test_call(&model_at(Fake::closed()), Duration::from_secs(10));
    failed_with(last(&lines), "unreachable");
}

#[test]
fn a_provider_that_does_not_answer_in_time_is_a_timeout() {
    let provider = Fake::start(vec![Reply::Hang]);
    let lines = test_call(&model_at(provider.addr), Duration::from_millis(400));
    failed_with(last(&lines), "timeout");
}

#[test]
fn no_model_set_is_a_plain_error_and_nothing_is_sent() {
    let lines = test_call(&Model::default(), Duration::from_secs(1));
    assert_eq!(last(&lines), &json!({ "error": "no model address is set" }));
}
