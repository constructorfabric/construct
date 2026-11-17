//! The provider's wire format: the request body written from the stored records, the answer
//! stream as bytes for the event parser, and the chunk of a streamed answer.

use std::io;

use bytes::Bytes;
use connectors::{Record, Role, ToolCall, ToolResult, Turn};
use oagw_sdk::Body;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

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

/// A list that may be missing or `null`: some gateways write `null` for what they have nothing to
/// say about, and a chunk that does not parse is skipped whole, text and all.
fn list_or_null<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Deserialize)]
pub(super) struct Chunk {
    #[serde(default, deserialize_with = "list_or_null")]
    pub(super) choices: Vec<Choice>,
}

#[derive(Deserialize)]
pub(super) struct Choice {
    #[serde(default)]
    pub(super) delta: Delta,
    #[serde(default)]
    pub(super) finish_reason: Option<String>,
}

/// Only `content` and the tool-call fragments are read: reasoning fields and the rest are ignored.
#[derive(Default, Deserialize)]
pub(super) struct Delta {
    #[serde(default)]
    pub(super) content: Option<String>,
    #[serde(default, deserialize_with = "list_or_null")]
    pub(super) tool_calls: Vec<Fragment>,
}

/// One piece of a tool call. The first piece of a call has its `id` and the function's name; the
/// later ones, which may have no `id`, carry more of the arguments text.
#[derive(Deserialize)]
pub(super) struct Fragment {
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<FunctionPart>,
}

#[derive(Deserialize)]
struct FunctionPart {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// Most calls one model turn may hold. It bounds the table the fragments build (a stream of tiny
/// fragments with new indexes would grow it without limit); more is a broken stream, not an answer.
const MAX_TURN_CALLS: usize = 64;

/// A tool call the model asked for, put together.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct PendingCall {
    pub(super) id: String,
    pub(super) name: String,
    /// The text as the model wrote it, never parsed: a parsed value would sort the keys.
    pub(super) arguments: String,
}

/// The fragments of one model turn, put together by `index`.
#[derive(Default)]
pub(super) struct Calls {
    calls: Vec<(usize, PendingCall)>,
    bytes: usize,
}

/// The fragments went past a limit.
#[derive(Debug)]
pub(super) struct TooMuch;

impl Calls {
    /// Add one fragment. The fragments of a call share its `index`; a fragment with an `id` that
    /// differs from the one the call has starts another call, so parallel calls that a provider
    /// numbers alike stay apart; a call that has no `id` yet takes the first one that comes. Without
    /// ids the fragments of one `index` are one call, since nothing tells them apart. A fragment
    /// with no `index` goes to the last call under the same `id` rule.
    ///
    /// # Errors
    /// When the turn holds too many calls or the arguments are longer than `limit` bytes in all.
    pub(super) fn push(&mut self, fragment: Fragment, limit: usize) -> Result<(), TooMuch> {
        let id = fragment.id.filter(|id| !id.is_empty());
        let position = match fragment.index {
            Some(index) => self.calls.iter().rposition(|(at, call)| {
                *at == index
                    && id
                        .as_ref()
                        .is_none_or(|id| call.id.is_empty() || call.id == *id)
            }),
            None => self.calls.len().checked_sub(1).filter(|&last| {
                let last_id = &self.calls[last].1.id;
                id.as_ref()
                    .is_none_or(|id| last_id.is_empty() || last_id == id)
            }),
        };
        let position = match position {
            Some(position) => position,
            None if self.calls.len() >= MAX_TURN_CALLS => return Err(TooMuch),
            None => {
                let at = fragment
                    .index
                    .unwrap_or_else(|| self.calls.iter().map(|(at, _)| at + 1).max().unwrap_or(0));
                self.calls.push((at, PendingCall::default()));
                self.calls.len() - 1
            }
        };
        let call = &mut self.calls[position].1;
        if let Some(id) = id {
            call.id = id;
        }
        if let Some(function) = fragment.function {
            // The name arrives whole in the first piece; a repeat is not appended to it.
            if let Some(name) = function.name.filter(|n| !n.is_empty())
                && call.name.is_empty()
            {
                call.name = name;
            }
            if let Some(arguments) = function.arguments {
                self.bytes += arguments.len();
                if self.bytes > limit {
                    return Err(TooMuch);
                }
                call.arguments.push_str(&arguments);
            }
        }
        Ok(())
    }

    pub(super) fn is_empty(&self) -> bool {
        self.calls.is_empty()
    }

    /// The calls in the order of their `index`.
    pub(super) fn finish(mut self) -> Vec<PendingCall> {
        self.calls.sort_by_key(|(index, _)| *index);
        self.calls.into_iter().map(|(_, call)| call).collect()
    }
}

// The request body. The field order of these structs is the order of the bytes.

#[derive(Serialize)]
struct RequestBody<'a> {
    model: &'a str,
    messages: Vec<Message<'a>>,
    stream: bool,
    /// After `stream`, and left out when there is no tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<&'a RawValue>,
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
/// providers want them. `tools` is the array of the tools offered, written once; it is left out
/// when there is none.
///
/// # Errors
/// Fails only when the body cannot be written as JSON.
pub fn request_body(
    model: &str,
    records: &[Record],
    tools: Option<&RawValue>,
) -> Result<String, serde_json::Error> {
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
        tools,
    })
}

#[cfg(test)]
// A failed `unwrap` in a test is a failed test.
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::chat::SOURCE;
    use connectors::Author;

    fn turn(role: Role, text: &str) -> Record {
        Record::Turn(Turn {
            tool: SOURCE.into(),
            session: "s".into(),
            time: "2026-11-13T10:00:00.000Z".into(),
            folder: String::new(),
            branch: String::new(),
            role,
            author: Author::Unknown,
            text: text.into(),
            tools: Vec::new(),
        })
    }

    fn fragment(value: serde_json::Value) -> Fragment {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn fragments_without_an_index_continue_the_last_call_or_start_one_with_an_id() {
        let mut calls = Calls::default();
        for value in [
            serde_json::json!({ "id": "a", "function": { "name": "f", "arguments": "{\"x\"" } }),
            serde_json::json!({ "function": { "arguments": ": 1}" } }),
            serde_json::json!({ "id": "b", "function": { "name": "g" } }),
        ] {
            calls.push(fragment(value), 1000).unwrap();
        }
        let calls = calls.finish();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].arguments, "{\"x\": 1}");
        assert_eq!((calls[1].id.as_str(), calls[1].name.as_str()), ("b", "g"));
        assert_eq!(calls[1].arguments, "");
    }

    #[test]
    fn a_call_without_an_id_takes_the_first_one_that_comes_with_or_without_an_index() {
        for index in [None, Some(0)] {
            let mut calls = Calls::default();
            let mut first = serde_json::json!({ "function": { "name": "f", "arguments": "{" } });
            let mut next = serde_json::json!({ "id": "a", "function": { "arguments": "}" } });
            if let Some(index) = index {
                first["index"] = index.into();
                next["index"] = index.into();
            }
            calls.push(fragment(first), 1000).unwrap();
            calls.push(fragment(next), 1000).unwrap();
            let calls = calls.finish();
            assert_eq!(calls.len(), 1);
            assert_eq!(
                (calls[0].id.as_str(), calls[0].arguments.as_str()),
                ("a", "{}")
            );
        }
    }

    #[test]
    fn too_many_calls_or_too_long_arguments_are_refused() {
        let mut calls = Calls::default();
        let long = serde_json::json!({ "index": 0, "function": { "arguments": "0123456789" } });
        assert!(calls.push(fragment(long), 5).is_err());
        let mut calls = Calls::default();
        let many = (0..=MAX_TURN_CALLS).map(|index| {
            calls.push(
                fragment(serde_json::json!({ "index": index, "id": "x" })),
                5,
            )
        });
        assert!(many.last().unwrap().is_err());
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
        let body = request_body("m", &records, None).unwrap();
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
        let body =
            request_body("m", &[call("a"), call("b"), result("a"), result("b")], None).unwrap();
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
        let body = request_body("m", &[turn(Role::User, "hi"), call], None).unwrap();
        assert!(
            !body.contains("orphan") && !body.contains("tool_calls"),
            "{body}"
        );
    }

    #[test]
    fn the_tools_array_follows_stream_as_written() {
        let tools = RawValue::from_string(r#"[{"type":"function"}]"#.to_string()).unwrap();
        let with = request_body("m", &[], Some(&tools)).unwrap();
        let without = request_body("m", &[], None).unwrap();
        assert_eq!(
            with,
            format!(
                "{},\"tools\":[{{\"type\":\"function\"}}]}}",
                &without[..without.len() - 1]
            )
        );
        assert!(without.ends_with(r#""stream":true}"#));
    }
}
