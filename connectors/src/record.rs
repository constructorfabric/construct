use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

/// Who said it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// One message of a session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    pub tool: String,
    pub session: String,
    /// RFC 3339, UTC (`2026-11-11T10:15:30Z`).
    pub time: String,
    pub folder: String,
    /// Empty when unknown.
    #[serde(default)]
    pub branch: String,
    pub role: Role,
    pub text: String,
}

/// A sign that a session was active at a time, without the text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub tool: String,
    pub session: String,
    /// RFC 3339, UTC.
    pub time: String,
    pub folder: String,
    /// Empty when unknown.
    #[serde(default)]
    pub branch: String,
}

/// A tool the model asked to run, in a chat. Written by the chat only, never by a reader.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// The source the session belongs to (`chat`).
    pub tool: String,
    pub session: String,
    /// RFC 3339, UTC.
    pub time: String,
    pub call_id: String,
    pub name: String,
    /// The arguments as the JSON text the model sent, byte for byte: never parsed and
    /// written again, because that would sort the keys and change the bytes.
    pub arguments: String,
}

/// What a tool answered, in a chat. Written by the chat only, never by a reader.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResult {
    /// The source the session belongs to (`chat`).
    pub tool: String,
    pub session: String,
    /// RFC 3339, UTC.
    pub time: String,
    pub call_id: String,
    pub text: String,
}

/// What a line of a session file turns into. On disk the variant is the `kind` field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Record {
    Turn(Turn),
    Mark(Mark),
    ToolCall(ToolCall),
    ToolResult(ToolResult),
}

impl Record {
    #[must_use]
    pub fn tool(&self) -> &str {
        match self {
            Record::Turn(t) => &t.tool,
            Record::Mark(m) => &m.tool,
            Record::ToolCall(c) => &c.tool,
            Record::ToolResult(r) => &r.tool,
        }
    }

    #[must_use]
    pub fn session(&self) -> &str {
        match self {
            Record::Turn(t) => &t.session,
            Record::Mark(m) => &m.session,
            Record::ToolCall(c) => &c.session,
            Record::ToolResult(r) => &r.session,
        }
    }

    #[must_use]
    pub fn time(&self) -> &str {
        match self {
            Record::Turn(t) => &t.time,
            Record::Mark(m) => &m.time,
            Record::ToolCall(c) => &c.time,
            Record::ToolResult(r) => &r.time,
        }
    }
}

/// The minute of a UTC RFC 3339 time (`2026-11-11T10:15`).
#[must_use]
pub fn minute_of(time: &str) -> &str {
    time.get(..16).unwrap_or(time)
}

/// Orders UTC RFC 3339 times, with or without a fraction of a second.
#[must_use]
pub fn compare_times(a: &str, b: &str) -> Ordering {
    fn split(t: &str) -> (&str, &str) {
        let (whole, rest) = t.split_at_checked(19).unwrap_or((t, ""));
        let fraction = rest.trim_start_matches('.').trim_end_matches('Z');
        (whole, fraction.trim_end_matches('0'))
    }
    split(a).cmp(&split(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_with_fractions_are_ordered() {
        let a = "2026-11-11T10:00:00Z";
        let b = "2026-11-11T10:00:00.5Z";
        assert_eq!(compare_times(a, b), Ordering::Less);
        assert_eq!(
            compare_times(a, "2026-11-11T10:00:00.000Z"),
            Ordering::Equal
        );
        assert_eq!(compare_times(b, "2026-11-11T10:00:01Z"), Ordering::Less);
        assert_eq!(minute_of(b), "2026-11-11T10:00");
    }

    #[test]
    fn tool_records_are_one_json_object_with_kind_and_keep_the_arguments_text() {
        let arguments = r#"{ "b": 1,  "a": 2 }"#;
        let call = Record::ToolCall(ToolCall {
            tool: "chat".into(),
            session: "s".into(),
            time: "2026-11-11T10:00:00.001Z".into(),
            call_id: "call_1".into(),
            name: "search".into(),
            arguments: arguments.into(),
        });
        let json = serde_json::to_string(&call).unwrap();
        assert!(!json.contains('\n') && json.contains(r#""kind":"toolcall""#));
        match serde_json::from_str::<Record>(&json).unwrap() {
            Record::ToolCall(c) => assert_eq!(c.arguments, arguments),
            other => panic!("{other:?}"),
        }
        let result = Record::ToolResult(ToolResult {
            tool: "chat".into(),
            session: "s".into(),
            time: "2026-11-11T10:00:00.002Z".into(),
            call_id: "call_1".into(),
            text: "line 1\nline 2".into(),
        });
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains('\n') && json.contains(r#""kind":"toolresult""#));
        assert_eq!(serde_json::from_str::<Record>(&json).unwrap(), result);
        assert_eq!(result.session(), "s");
        assert_eq!(call.tool(), "chat");
    }

    #[test]
    fn turn_is_one_json_object_with_kind() {
        let r = Record::Turn(Turn {
            tool: "t".into(),
            session: "s".into(),
            time: "2026-11-11T10:00:00Z".into(),
            folder: "/f".into(),
            branch: String::new(),
            role: Role::User,
            text: "hi".into(),
        });
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains(r#""kind":"turn""#) && json.contains(r#""role":"user""#));
        assert_eq!(serde_json::from_str::<Record>(&json).unwrap(), r);
    }
}
