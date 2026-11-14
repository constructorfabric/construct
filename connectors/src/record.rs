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

/// Whether `time` is a UTC RFC 3339 time as `minute_of` and `compare_times` expect:
/// `2026-11-11T10:15:30Z`, optionally with a fraction of a second. Times with an offset
/// (`+02:00`) are not accepted: they would order wrongly.
#[must_use]
pub fn is_utc_time(time: &str) -> bool {
    let Some(body) = time.strip_suffix('Z') else {
        return false;
    };
    let (whole, fraction) = match body.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (body, None),
    };
    let digits_at = |s: &str, at: &[usize]| at.iter().all(|i| s.as_bytes()[*i].is_ascii_digit());
    whole.len() == 19
        && whole.is_ascii()
        && digits_at(whole, &[0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18])
        && [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')]
            .iter()
            .all(|(i, c)| whole.as_bytes()[*i] == *c)
        && fraction.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
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
    fn only_utc_times_are_accepted() {
        assert!(is_utc_time("2026-11-11T10:15:30Z"));
        assert!(is_utc_time("2026-11-11T10:15:30.123456Z"));
        for bad in [
            "",
            "2026-11-11T10:15:30",
            "2026-11-11T10:15:30+02:00",
            "2026-11-11 10:15:30Z",
            "2026-11-11T10:15Z",
            "2026-11-11T10:15:30.Z",
            "2026-11-11T10:15:3éZ",
        ] {
            assert!(!is_utc_time(bad), "{bad}");
        }
    }

    #[test]
    fn wire_format_is_pinned() {
        let turn = Record::Turn(Turn {
            tool: "t".into(),
            session: "s".into(),
            time: "2026-11-11T10:00:00Z".into(),
            folder: "/f".into(),
            branch: "main".into(),
            role: Role::Assistant,
            text: "hi".into(),
        });
        let json = r#"{"kind":"turn","tool":"t","session":"s","time":"2026-11-11T10:00:00Z","folder":"/f","branch":"main","role":"assistant","text":"hi"}"#;
        assert_eq!(serde_json::to_string(&turn).unwrap(), json);
        assert_eq!(serde_json::from_str::<Record>(json).unwrap(), turn);

        let mark = Record::Mark(Mark {
            tool: "t".into(),
            session: "s".into(),
            time: "2026-11-11T10:00:00Z".into(),
            folder: "/f".into(),
            branch: String::new(),
        });
        let json = r#"{"kind":"mark","tool":"t","session":"s","time":"2026-11-11T10:00:00Z","folder":"/f","branch":""}"#;
        assert_eq!(serde_json::to_string(&mark).unwrap(), json);
        assert_eq!(serde_json::from_str::<Record>(json).unwrap(), mark);
    }

    #[test]
    fn a_record_without_a_branch_is_refused() {
        let json = r#"{"kind":"mark","tool":"t","session":"s","time":"2026-11-11T10:00:00Z","folder":"/f"}"#;
        assert!(serde_json::from_str::<Record>(json).is_err());
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
