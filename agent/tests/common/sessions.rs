//! Builders of the sessions the time rules take, for the tests of the dashboard numbers.
#![allow(clippy::unwrap_used, dead_code)]

use construct_agent::activity::rules::{SessionMinutes, minute_number};

/// The minute number of a UTC time such as `2026-11-10T09:00`.
pub fn at(time: &str) -> i64 {
    minute_number(time).unwrap()
}

/// A session of the user's own, with its active minutes.
pub fn session(
    tool: &str,
    name: &str,
    project: &str,
    ticket: &str,
    minutes: &[i64],
) -> SessionMinutes {
    SessionMinutes {
        tool: tool.to_owned(),
        session: name.to_owned(),
        project: project.to_owned(),
        ticket: ticket.to_owned(),
        unattended: false,
        minutes: minutes.to_vec(),
    }
}

/// `count` active minutes in a row from `start`.
pub fn run(start: i64, count: i64) -> Vec<i64> {
    (start..start + count).collect()
}
