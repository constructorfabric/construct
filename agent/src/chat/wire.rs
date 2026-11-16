//! The provider's wire format: the request body written from the stored records, the answer
//! stream as bytes for the event parser, and the chunk of a streamed answer.

use std::io;

use bytes::Bytes;
use connectors::{Record, Role, ToolCall, ToolResult, Turn};
use oagw_sdk::Body;
use serde::{Deserialize, Serialize};

use super::INSTRUCTION;

/// The response as an `http` response over a byte stream made of `Response::chunk()`. The stream
/// ends with a [`WireCap`] error at `limit` bytes: the parser does not limit its buffer.
pub(super) fn into_http(response: reqwest::Response, limit: usize) -> http::Response<Body> {
    let status = response.status();
    let headers = response.headers().clone();
    let stream = futures_util::stream::unfold(Some((response, 0_usize)), move |state| async move {
        let (mut response, total) = state?;
        match response.chunk().await {
            Ok(Some(chunk)) => {
                let total = total + chunk.len();
                if total > limit {
                    return Some((Err(Box::new(WireCap) as oagw_sdk::body::BoxError), None));
                }
                Some((
                    Ok::<Bytes, oagw_sdk::body::BoxError>(chunk),
                    Some((response, total)),
                ))
            }
            Ok(None) => None,
            Err(_) => Some((Err(stream_error()), None)),
        }
    });
    let mut out = http::Response::new(Body::Stream(Box::pin(stream)));
    *out.status_mut() = status;
    *out.headers_mut() = headers;
    out
}

/// The error of a stream that went past the wire cap.
#[derive(Debug)]
pub(super) struct WireCap;

impl std::fmt::Display for WireCap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the answer stream is over its size cap")
    }
}

impl std::error::Error for WireCap {}

fn stream_error() -> oagw_sdk::body::BoxError {
    Box::new(io::Error::other("the answer stream broke"))
}

#[derive(Deserialize)]
pub(super) struct Chunk {
    #[serde(default)]
    pub(super) choices: Vec<Choice>,
}

#[derive(Deserialize)]
pub(super) struct Choice {
    #[serde(default)]
    pub(super) delta: Delta,
    #[serde(default)]
    pub(super) finish_reason: Option<String>,
}

/// Only `content` is text: reasoning fields and the rest are ignored.
#[derive(Default, Deserialize)]
pub(super) struct Delta {
    #[serde(default)]
    pub(super) content: Option<String>,
}

// The request body. The field order of these structs is the order of the bytes.

#[derive(Serialize)]
struct RequestBody<'a> {
    model: &'a str,
    messages: Vec<Message<'a>>,
    stream: bool,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'static str,
    content: Option<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<WireCall<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<&'a str>,
}

#[derive(Serialize)]
struct WireCall<'a> {
    id: &'a str,
    r#type: &'static str,
    function: WireFunction<'a>,
}

#[derive(Serialize)]
struct WireFunction<'a> {
    name: &'a str,
    /// The stored text, byte for byte.
    arguments: &'a str,
}

fn plain<'a>(role: &'static str, content: &'a str) -> Message<'a> {
    Message {
        role,
        content: Some(content),
        tool_calls: Vec::new(),
        tool_call_id: None,
    }
}

/// The request body for one answer: the fixed instruction, then every record of the
/// conversation in order. Tool calls that follow each other are one assistant message, as the
/// providers want them.
///
/// # Errors
/// Fails only when the body cannot be written as JSON.
pub fn request_body(model: &str, records: &[Record]) -> Result<String, serde_json::Error> {
    let mut messages = vec![plain("system", INSTRUCTION)];
    for (at, record) in records.iter().enumerate() {
        match record {
            Record::Turn(Turn { role, text, .. }) => messages.push(plain(
                match role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                },
                text,
            )),
            Record::ToolCall(ToolCall {
                call_id,
                name,
                arguments,
                ..
            }) => {
                // A call whose result was never stored (a crash between the two writes) is not
                // sent: the providers refuse a call with no reply.
                if !records[at..]
                    .iter()
                    .any(|r| matches!(r, Record::ToolResult(t) if t.call_id == *call_id))
                {
                    continue;
                }
                let call = WireCall {
                    id: call_id,
                    r#type: "function",
                    function: WireFunction { name, arguments },
                };
                match messages.last_mut() {
                    Some(last) if last.role == "assistant" && !last.tool_calls.is_empty() => {
                        last.tool_calls.push(call);
                    }
                    _ => messages.push(Message {
                        role: "assistant",
                        content: None,
                        tool_calls: vec![call],
                        tool_call_id: None,
                    }),
                }
            }
            Record::ToolResult(ToolResult { call_id, text, .. }) => messages.push(Message {
                role: "tool",
                content: Some(text),
                tool_calls: Vec::new(),
                tool_call_id: Some(call_id),
            }),
            Record::Mark(_) => {}
        }
    }
    serde_json::to_string(&RequestBody {
        model,
        messages,
        stream: true,
    })
}

#[cfg(test)]
// A failed `unwrap` in a test is a failed test.
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::chat::SOURCE;

    fn turn(role: Role, text: &str) -> Record {
        Record::Turn(Turn {
            tool: SOURCE.into(),
            session: "s".into(),
            time: "2026-11-13T10:00:00.000Z".into(),
            folder: String::new(),
            branch: String::new(),
            role,
            text: text.into(),
        })
    }

    #[test]
    fn body_has_a_fixed_field_order_and_the_stored_arguments_text() {
        let arguments = r#"{ "z":1,   "a" : [2,1] }"#;
        let records = vec![
            turn(Role::User, "hi \"there\""),
            Record::ToolCall(ToolCall {
                tool: SOURCE.into(),
                session: "s".into(),
                time: "2026-11-13T10:00:01.000Z".into(),
                call_id: "call_1".into(),
                name: "recall_facts".into(),
                arguments: arguments.into(),
            }),
            Record::ToolResult(ToolResult {
                tool: SOURCE.into(),
                session: "s".into(),
                time: "2026-11-13T10:00:02.000Z".into(),
                call_id: "call_1".into(),
                text: "none".into(),
            }),
            turn(Role::Assistant, "ok"),
        ];
        let body = request_body("m", &records).unwrap();
        let escaped = serde_json::to_string(arguments).unwrap();
        let expected = format!(
            concat!(
                r#"{{"model":"m","messages":[{{"role":"system","content":{instruction}}},"#,
                r#"{{"role":"user","content":"hi \"there\""}},"#,
                r#"{{"role":"assistant","content":null,"tool_calls":[{{"id":"call_1","type":"function","function":{{"name":"recall_facts","arguments":{arguments}}}}}]}},"#,
                r#"{{"role":"tool","content":"none","tool_call_id":"call_1"}},"#,
                r#"{{"role":"assistant","content":"ok"}}],"stream":true}}"#
            ),
            instruction = serde_json::to_string(INSTRUCTION).unwrap(),
            arguments = escaped,
        );
        assert_eq!(body, expected);
    }

    #[test]
    fn calls_that_follow_each_other_are_one_assistant_message() {
        let call = |id: &str| {
            Record::ToolCall(ToolCall {
                tool: SOURCE.into(),
                session: "s".into(),
                time: "2026-11-13T10:00:01.000Z".into(),
                call_id: id.into(),
                name: "f".into(),
                arguments: "{}".into(),
            })
        };
        let result = |id: &str| {
            Record::ToolResult(ToolResult {
                tool: SOURCE.into(),
                session: "s".into(),
                time: "2026-11-13T10:00:02.000Z".into(),
                call_id: id.into(),
                text: "r".into(),
            })
        };
        let body = request_body("m", &[call("a"), call("b"), result("a"), result("b")]).unwrap();
        assert_eq!(body.matches("\"tool_calls\"").count(), 1);
        assert!(body.contains(r#""id":"a""#) && body.contains(r#""id":"b""#));
    }

    #[test]
    fn a_call_with_no_result_is_not_sent() {
        let call = Record::ToolCall(ToolCall {
            tool: SOURCE.into(),
            session: "s".into(),
            time: "2026-11-13T10:00:01.000Z".into(),
            call_id: "orphan".into(),
            name: "recall_facts".into(),
            arguments: "{}".into(),
        });
        let body = request_body("m", &[turn(Role::User, "hi"), call]).unwrap();
        assert!(
            !body.contains("orphan") && !body.contains("tool_calls"),
            "{body}"
        );
    }
}
