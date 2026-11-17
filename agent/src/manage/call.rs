//! One HTTP call to the route, and how its answer is read.
//!
//! The route may have run the planner and written facts when an answer is lost or says its result
//! is unknown, so only an answer that proves nothing was done lets a prompt be sent again.

use std::io::ErrorKind;
use std::time::Duration;

use serde_json::Value;

use crate::host::{Connection, loopback_agent};

/// What the route answered with `200`.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Answer {
    /// Facts were stored.
    Stored { added: u64 },
    /// Nothing to store, or a reason that a new call would not change.
    Nothing,
    /// The call did not end in an answer worth keeping; a new call may.
    Failed(String),
}

/// Reasons a nothing-stored answer can give that a second call may get past.
const TRANSIENT: [&str; 7] = [
    "timeout",
    "model_failed",
    "processing_failed",
    "rerun_limit",
    "no_planner",
    "check_failed",
    "erasure_in_progress",
];

/// What the gear's `503` says when a write was sent and its result is not known: the start of
/// `WRITE_UNKNOWN_DETAIL` in the gear's `api/rest/error.rs`.
const WRITE_UNKNOWN: &str = "result is unknown";

/// Statuses that refuse one prompt as such (a body or a field the route does not take). Any other
/// `4xx` or `501` is taken for a refusal of every call: no token, no right, no such subject, no
/// processing, or a route that changed. A new call of any prompt would get the same.
const PROMPT_REFUSES: [u16; 3] = [400, 413, 422];

fn read_answer(body: &str) -> Answer {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return Answer::Failed("the answer is not JSON".into());
    };
    match value["outcome"].as_str() {
        Some("stored") => Answer::Stored {
            added: value["added"].as_u64().unwrap_or(0),
        },
        Some("nothing_changed") => {
            let reason = value["reason"].as_str().unwrap_or("unknown");
            if TRANSIENT.contains(&reason) {
                Answer::Failed(format!("the host stored nothing: {reason}"))
            } else {
                Answer::Nothing
            }
        }
        _ => Answer::Failed("the answer has no outcome".into()),
    }
}

/// How one call ended.
pub(super) enum Reply {
    Answered(Answer),
    /// The call did not get to the host, or the host did nothing: no connection, a plain `503`,
    /// `429`. The same prompt may be sent again.
    NotDelivered(String),
    /// The host may have run the prompt and stored facts, and the agent does not know: the wait
    /// ran out, the connection broke, `500`, `502`, `504`, or a `503` that says the result of the
    /// write is unknown. The prompt is not sent again.
    MaybeProcessed,
    /// The host refuses every call of the agent; the prompt is not marked as sent.
    HostRefused(String),
    /// The host refuses this prompt (see [`PROMPT_REFUSES`]); another prompt may be taken.
    PromptRefused(String),
}

pub(super) fn call(connection: &Connection, body: &str, timeout: Duration) -> Reply {
    let sent = loopback_agent(timeout)
        .post(&connection.manage_url())
        .header("Authorization", format!("Bearer {}", connection.token))
        .header("Content-Type", "application/json")
        .send(body);
    let mut response = match sent {
        Ok(response) => response,
        Err(ureq::Error::ConnectionFailed | ureq::Error::HostNotFound) => {
            return Reply::NotDelivered("no connection to the host".into());
        }
        Err(ureq::Error::Io(e))
            if matches!(
                e.kind(),
                ErrorKind::ConnectionRefused | ErrorKind::AddrNotAvailable
            ) =>
        {
            return Reply::NotDelivered("no connection to the host".into());
        }
        Err(_) => return Reply::MaybeProcessed,
    };
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string();
    match status {
        200 => text.map_or(Reply::MaybeProcessed, |text| {
            Reply::Answered(read_answer(&text))
        }),
        // A `503` whose text cannot be read may be the write-unknown one: it is not sent again.
        503 if text.as_ref().map_or(true, |t| t.contains(WRITE_UNKNOWN)) => Reply::MaybeProcessed,
        503 | 429 => Reply::NotDelivered(format!("the host answered {status}")),
        500..=599 => Reply::MaybeProcessed,
        s if PROMPT_REFUSES.contains(&s) => Reply::PromptRefused(format!("the host answered {s}")),
        s => Reply::HostRefused(format!("the host answered {s}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_of_the_write_unknown_answer_is_the_gears() {
        // The gear's detail begins "The write was sent, but its result is unknown: ".
        assert!(
            "The write was sent, but its result is unknown: it may have been stored."
                .contains(WRITE_UNKNOWN)
        );
    }

    #[test]
    fn answers_are_sorted_into_stored_nothing_and_failed() {
        let stored = r#"{"outcome":"stored","reason":null,"added":2,"replaced":0,"removed":0}"#;
        assert_eq!(read_answer(stored), Answer::Stored { added: 2 });
        let nothing = |reason: &str| {
            read_answer(&format!(
                r#"{{"outcome":"nothing_changed","reason":"{reason}","added":0,"replaced":0,"removed":0}}"#
            ))
        };
        assert_eq!(nothing("no_change"), Answer::Nothing);
        assert_eq!(nothing("personalization_off"), Answer::Nothing);
        assert_eq!(nothing("token_cap"), Answer::Nothing);
        assert!(matches!(nothing("timeout"), Answer::Failed(_)));
        assert!(matches!(nothing("model_failed"), Answer::Failed(_)));
        assert!(matches!(read_answer("nope"), Answer::Failed(_)));
    }
}
