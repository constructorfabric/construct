//! Reading the request bodies a scripted provider noted.
#![allow(clippy::unwrap_used, dead_code)]

use serde_json::Value;

/// The messages of a request body.
pub fn messages(body: &Value) -> Vec<Value> {
    body["messages"].as_array().unwrap().clone()
}

/// The names of the tools a request body offers, in order.
pub fn names(body: &Value) -> Vec<String> {
    body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap().to_string())
        .collect()
}

/// The results of the calls in a request body, in order.
pub fn results(body: &Value) -> Vec<String> {
    messages(body)
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["content"].as_str().unwrap().to_string())
        .collect()
}

/// The names of the calls in a request body, in order.
pub fn calls(body: &Value) -> Vec<String> {
    messages(body)
        .iter()
        .flat_map(|m| m["tool_calls"].as_array().cloned().unwrap_or_default())
        .map(|call| call["function"]["name"].as_str().unwrap().to_string())
        .collect()
}
