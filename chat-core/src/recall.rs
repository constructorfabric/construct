//! The `recall_facts` result: the person's profile from the Construct host as one text.
//!
//! The text is the only thing compared with the last result in the conversation, so the same
//! profile must always give the same bytes. Only the reason a read failed is logged, never what was
//! read: the body and the text hold the person's facts.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use connectors::Record;
use serde::Deserialize;
use tokio::time::timeout;

/// The name of the tool the facts come as.
pub const NAME: &str = "recall_facts";
/// The result when the all-collection switch is off.
pub(super) const MEMORY_OFF: &str = "Error: memory is off";
/// The result when there is nothing to tell, and what no call yet stands for.
pub(super) const NO_FACTS: &str = "No facts.";
const HEADER: &str =
    "These are the facts known about the user. This list replaces all earlier lists of facts.";
/// The whole read, connect to last byte.
const DEADLINE: Duration = Duration::from_secs(1);
/// Most bytes of a profile body a host reads for [`Facts::profile`].
pub const BODY_LIMIT: usize = 1024 * 1024;
/// Most bytes of the facts text. It is stored once and replayed in every later request, and the
/// gear does not bound a profile, so the facts past this (in the fixed order) are left out.
const TEXT_LIMIT: usize = 32 * 1024;
/// Most characters of one fact's value, so one fact always fits under [`TEXT_LIMIT`].
const VALUE_LIMIT: usize = 2048;
/// The end of a value that was cut at [`VALUE_LIMIT`].
pub(super) const VALUE_CUT: &str = " [cut]";
/// The last line of a text that was cut, so the model knows the list is not whole.
const CUT_LINE: &str = "\n(The list is cut: more facts are known than are shown.)";

#[derive(Deserialize)]
struct Profile {
    categories: Vec<Category>,
}

#[derive(Deserialize)]
struct Category {
    entity_kind: String,
    facts: Vec<Fact>,
}

#[derive(Deserialize)]
struct Fact {
    property: String,
    value: serde_json::Value,
    stored_at: String,
}

/// What the host gives the chat as the person's facts.
pub trait Facts: Send + Sync {
    /// Whether the person's facts may be read at all: the all-collection switch. While it is off the
    /// chat tells the model that memory is off.
    fn enabled(&self) -> bool;

    /// The person's profile as the Construct host answers it (a JSON body with `categories`), read
    /// with `client`; `Ok(None)` when the host is not ready. The chat bounds the wait to one second
    /// and only logs the reason of an `Err`, never what was read.
    ///
    /// # Errors
    /// The reason the read failed, in a few plain words and without any of the body.
    fn profile<'a>(&'a self, client: &'a reqwest::Client) -> ProfileRead<'a>;
}

/// The future of [`Facts::profile`].
pub type ProfileRead<'a> =
    Pin<Box<dyn Future<Output = Result<Option<Vec<u8>>, String>> + Send + 'a>>;

/// The facts of the person, or [`NO_FACTS`] when Construct is not ready, answers with anything
/// but 200, has nothing, or is slower than the deadline. A failure is logged by its reason only,
/// so an operator can tell a dead host from a person with no facts; never by what was read.
pub(super) async fn facts(client: &reqwest::Client, source: &dyn Facts) -> String {
    match timeout(DEADLINE, source.profile(client)).await {
        Err(_) => eprintln!("chat: recall: timeout"),
        Ok(Err(reason)) => eprintln!("chat: recall: {reason}"),
        Ok(Ok(None)) => eprintln!("chat: recall: construct not ready"),
        Ok(Ok(Some(body))) => match render(&body) {
            Ok(Some(text)) => return text,
            Ok(None) => {}
            Err(_) => eprintln!("chat: recall: not a profile"),
        },
    }
    NO_FACTS.into()
}

/// The text of a profile body: a first line, then one `category.property: value` line per fact
/// ordered by category, property, `stored_at`, value. `Ok(None)` when there is no fact. A text
/// over [`TEXT_LIMIT`] ends in [`CUT_LINE`].
///
/// # Errors
/// The body is not a profile.
pub fn render(body: &[u8]) -> Result<Option<String>, serde_json::Error> {
    let profile: Profile = serde_json::from_slice(body)?;
    let mut lines: Vec<(String, String, String, String)> = profile
        .categories
        .into_iter()
        .flat_map(|c| {
            let kind = c.entity_kind;
            c.facts.into_iter().map(move |f| {
                let value = match f.value {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                };
                let value_len = value.chars().count();
                // One fact is one line.
                let mut value: String = value
                    .chars()
                    .take(VALUE_LIMIT)
                    .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
                    .collect();
                // A value that was cut says so: the model must not read half of it as the whole.
                if value.chars().count() == VALUE_LIMIT && value_len > VALUE_LIMIT {
                    value.push_str(VALUE_CUT);
                }
                (kind.clone(), f.property, f.stored_at, value)
            })
        })
        .collect();
    if lines.is_empty() {
        return Ok(None);
    }
    lines.sort();
    let mut text = HEADER.to_string();
    let count = lines.len();
    for (at, (kind, property, _, value)) in lines.into_iter().enumerate() {
        let line = format!("\n{kind}.{property}: {value}");
        // Room is kept for the cut line, except when this is the last fact and nothing is left
        // out: then it only has to fit itself.
        let reserve = if at + 1 == count { 0 } else { CUT_LINE.len() };
        if text.len() + line.len() + reserve > TEXT_LIMIT {
            text.push_str(CUT_LINE);
            break;
        }
        text.push_str(&line);
    }
    Ok(Some(text))
}

/// The text of the last `recall_facts` result in the records; [`NO_FACTS`] when there is none.
/// A call with no result (a crash between the two writes) does not count.
pub(super) fn last_result(records: &[Record]) -> &str {
    records
        .iter()
        .rev()
        .find_map(|r| match r {
            Record::ToolResult(t)
                if records.iter().any(
                    |c| matches!(c, Record::ToolCall(c) if c.name == NAME && c.call_id == t.call_id),
                ) =>
            {
                Some(t.text.as_str())
            }
            _ => None,
        })
        .unwrap_or(NO_FACTS)
}

#[cfg(test)]
// A failed `unwrap` in a test is a failed test.
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn facts_come_in_a_fixed_order_whatever_the_body_order() {
        let a = r#"{"categories":[
            {"entity_kind":"skill","facts":[{"property":"skills","value":{"name":"Rust"},"stored_at":"2026-01-02T00:00:00Z"}]},
            {"entity_kind":"identity","facts":[
                {"property":"name","value":"Ann","stored_at":"2026-01-03T00:00:00Z"},
                {"property":"name","value":"Anna","stored_at":"2026-01-01T00:00:00Z"}]}]}"#;
        let b = r#"{"categories":[
            {"entity_kind":"identity","facts":[
                {"property":"name","value":"Anna","stored_at":"2026-01-01T00:00:00Z"},
                {"property":"name","value":"Ann","stored_at":"2026-01-03T00:00:00Z"}]},
            {"entity_kind":"skill","facts":[{"property":"skills","value":{"name":"Rust"},"stored_at":"2026-01-02T00:00:00Z"}]}]}"#;
        let text = render(a.as_bytes()).unwrap().unwrap();
        assert_eq!(text, render(b.as_bytes()).unwrap().unwrap());
        assert_eq!(
            text,
            format!(
                "{HEADER}\nidentity.name: Anna\nidentity.name: Ann\nskill.skills: {{\"name\":\"Rust\"}}"
            )
        );
    }

    #[test]
    fn no_facts_and_no_profile_render_nothing() {
        assert_eq!(render(br#"{"categories":[]}"#).unwrap(), None);
        assert_eq!(
            render(br#"{"categories":[{"entity_kind":"role","facts":[]}]}"#).unwrap(),
            None
        );
        assert!(render(b"nope").is_err());
        assert!(render(br#"{"categories":[{"entity_kind":"role"}]}"#).is_err());
    }

    #[test]
    fn a_huge_profile_is_cut_at_whole_lines() {
        let facts: Vec<String> = (0..2000)
            .map(|i| {
                format!(
                    r#"{{"property":"p{i:04}","value":"{}","stored_at":"t"}}"#,
                    "x".repeat(100)
                )
            })
            .collect();
        let body = format!(
            r#"{{"categories":[{{"entity_kind":"skill","facts":[{}]}}]}}"#,
            facts.join(",")
        );
        let text = render(body.as_bytes()).unwrap().unwrap();
        assert!(text.len() <= TEXT_LIMIT);
        assert!(text.ends_with(CUT_LINE));
        assert!(text.contains("skill.p0000") && !text.contains("skill.p1999"));
    }

    #[test]
    fn one_huge_fact_is_shortened_and_never_leaves_the_header_alone() {
        let body = format!(
            r#"{{"categories":[{{"entity_kind":"skill","facts":[{{"property":"p","value":"{}","stored_at":"t"}}]}}]}}"#,
            "é".repeat(100_000)
        );
        let text = render(body.as_bytes()).unwrap().unwrap();
        assert!(text.len() <= TEXT_LIMIT);
        assert!(text.contains("skill.p: éé") && text.ends_with(VALUE_CUT));
        assert!(!text.ends_with(CUT_LINE));
    }

    /// A profile of `n` facts whose lines are 100 bytes each, the last one `extra` bytes longer,
    /// and with `tail` one more short fact after them.
    fn sized(n: usize, extra: usize, tail: bool) -> String {
        let mut facts: Vec<String> = (0..n)
            .map(|i| {
                let pad = 86 + if i + 1 == n { extra } else { 0 };
                format!(
                    r#"{{"property":"p{i:04}","value":"{}","stored_at":"t"}}"#,
                    "x".repeat(pad)
                )
            })
            .collect();
        if tail {
            facts.push(r#"{"property":"p9999","value":"y","stored_at":"t"}"#.into());
        }
        format!(
            r#"{{"categories":[{{"entity_kind":"skill","facts":[{}]}}]}}"#,
            facts.join(",")
        )
    }

    #[test]
    fn a_value_is_marked_only_when_it_is_over_the_value_limit() {
        let text = |n: usize| {
            let body = format!(
                r#"{{"categories":[{{"entity_kind":"skill","facts":[{{"property":"p","value":"{}","stored_at":"t"}}]}}]}}"#,
                "x".repeat(n)
            );
            render(body.as_bytes()).unwrap().unwrap()
        };
        let exact = text(VALUE_LIMIT);
        assert!(exact.ends_with(&"x".repeat(VALUE_LIMIT)) && !exact.contains(VALUE_CUT));
        let over = text(VALUE_LIMIT + 1);
        assert!(over.ends_with(&format!("{}{VALUE_CUT}", "x".repeat(VALUE_LIMIT))));
    }

    #[test]
    fn the_cut_line_margin_keeps_a_cut_text_under_the_cap() {
        // n whole lines end exactly 10 bytes under the cap: closer than the cut line is long,
        // so with one more fact the last whole line has to go too.
        let n = (TEXT_LIMIT - 10 - HEADER.len()) / 100;
        let extra = TEXT_LIMIT - 10 - HEADER.len() - 100 * n;
        let fits = render(sized(n, extra, false).as_bytes()).unwrap().unwrap();
        assert_eq!(fits.len(), TEXT_LIMIT - 10);
        assert!(
            !fits.ends_with(CUT_LINE),
            "nothing is left out: no cut line"
        );

        let cut = render(sized(n, extra, true).as_bytes()).unwrap().unwrap();
        assert!(cut.len() <= TEXT_LIMIT, "{}", cut.len());
        assert!(cut.ends_with(CUT_LINE));
        assert!(
            !cut.contains(&format!("skill.p{:04}", n - 1)),
            "the last whole line was dropped"
        );
        assert!(cut.contains(&format!("skill.p{:04}", n - 2)));
    }

    #[test]
    fn an_orphan_call_does_not_count_as_the_last_result() {
        use connectors::{ToolCall, ToolResult};
        let call = |id: &str| {
            Record::ToolCall(ToolCall {
                tool: "chat".into(),
                session: "s".into(),
                time: "t".into(),
                call_id: id.into(),
                name: NAME.into(),
                arguments: "{}".into(),
            })
        };
        let result = Record::ToolResult(ToolResult {
            tool: "chat".into(),
            session: "s".into(),
            time: "t".into(),
            call_id: "a".into(),
            text: "facts A".into(),
        });
        assert_eq!(last_result(&[]), NO_FACTS);
        assert_eq!(
            last_result(&[call("a"), result.clone(), call("orphan")]),
            "facts A"
        );
        assert_eq!(last_result(&[call("orphan")]), NO_FACTS);
    }
}
