//! The calls to the host's routes for the person's own settings and erasure.
//!
//! Every failure is a fixed plain reason: the answer of the host is never copied into one, so no
//! fact or prompt can reach a log line or a reply.

use std::io::ErrorKind;
use std::time::Duration;

use serde_json::{Value, json};

use crate::host::{Connection, loopback_agent};

/// How long one of these calls may take; the host answers the erasure at once.
const TIMEOUT: Duration = Duration::from_secs(15);

/// What the host's settings route says about the person.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Gear {
    pub personalization_enabled: bool,
    pub erasure_in_progress: bool,
}

/// Read the settings of the person in the host.
pub(super) fn read(connection: &Connection) -> Result<Gear, String> {
    let sent = loopback_agent(TIMEOUT)
        .get(&connection.subject_url("settings"))
        .header("Authorization", format!("Bearer {}", connection.token))
        .call();
    answer(sent, 200)
}

/// Turn personalization on in the host.
pub(super) fn turn_personalization_on(connection: &Connection) -> Result<(), String> {
    let sent = loopback_agent(TIMEOUT)
        .put(&connection.subject_url("settings"))
        .header("Authorization", format!("Bearer {}", connection.token))
        .header("Content-Type", "application/json")
        .send(json!({ "personalization_enabled": true }).to_string());
    status_only(sent, 200)
}

/// Ask the host to erase the person. It answers `202` and erases in the background.
pub(super) fn request_erasure(connection: &Connection) -> Result<(), String> {
    let sent = loopback_agent(TIMEOUT)
        .post(&connection.subject_url("erasure"))
        .header("Authorization", format!("Bearer {}", connection.token))
        .send_empty();
    status_only(sent, 202)
}

/// Only the status counts: the erasure cannot be undone, so a body of another shape must not make
/// a request that went through look like one that did not.
fn status_only(
    sent: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    expected: u16,
) -> Result<(), String> {
    checked(sent, expected).map(|_| ())
}

fn answer(
    sent: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    expected: u16,
) -> Result<Gear, String> {
    let mut response = checked(sent, expected)?;
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|_| "the Construct host sent an unreadable answer".to_string())?;
    read_gear(&text).ok_or_else(|| "the Construct host sent an unreadable answer".to_string())
}

fn checked(
    sent: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    expected: u16,
) -> Result<ureq::http::Response<ureq::Body>, String> {
    let response = match sent {
        Ok(response) => response,
        Err(ureq::Error::ConnectionFailed | ureq::Error::HostNotFound) => {
            return Err(unreachable_reason());
        }
        Err(ureq::Error::Io(e))
            if matches!(
                e.kind(),
                ErrorKind::ConnectionRefused | ErrorKind::AddrNotAvailable
            ) =>
        {
            return Err(unreachable_reason());
        }
        Err(_) => return Err("the Construct host did not answer in time".into()),
    };
    let status = response.status().as_u16();
    if status != expected {
        return Err(format!("the Construct host answered {status}"));
    }
    Ok(response)
}

pub(super) fn unreachable_reason() -> String {
    "the Construct host cannot be reached".into()
}

fn read_gear(body: &str) -> Option<Gear> {
    let value: Value = serde_json::from_str(body).ok()?;
    Some(Gear {
        personalization_enabled: value["personalization_enabled"].as_bool()?,
        erasure_in_progress: value["erasure_in_progress"].as_bool()?,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_answer_is_read_and_a_partial_one_is_not() {
        let gear = read_gear(
            r#"{"subject_id":"s","personalization_enabled":false,"erasure_in_progress":true}"#,
        )
        .unwrap();
        assert_eq!(
            gear,
            Gear {
                personalization_enabled: false,
                erasure_in_progress: true
            }
        );
        assert_eq!(read_gear(r#"{"personalization_enabled":true}"#), None);
        assert_eq!(read_gear("nope"), None);
    }
}
