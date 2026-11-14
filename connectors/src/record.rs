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

/// What a line of a session file turns into. On disk the variant is the `kind` field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Record {
    Turn(Turn),
    Mark(Mark),
}

impl Record {
    #[must_use]
    pub fn tool(&self) -> &str {
        match self {
            Record::Turn(t) => &t.tool,
            Record::Mark(m) => &m.tool,
        }
    }

    #[must_use]
    pub fn session(&self) -> &str {
        match self {
            Record::Turn(t) => &t.session,
            Record::Mark(m) => &m.session,
        }
    }

    #[must_use]
    pub fn time(&self) -> &str {
        match self {
            Record::Turn(t) => &t.time,
            Record::Mark(m) => &m.time,
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
