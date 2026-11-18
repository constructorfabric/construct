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

/// Sessions in three projects over the 9th to the 11th of November 2026: `/w/shop` and `/w/blog`
/// overlap in time on the 10th, and `/w/lab` is a script's.
pub fn fixture() -> Vec<SessionMinutes> {
    let mut lab = session(
        "claude",
        "lab",
        "/w/lab",
        "LAB-1",
        &run(at("2026-11-10T09:00"), 30),
    );
    lab.unattended = true;
    vec![
        session(
            "claude",
            "shop-1",
            "/w/shop",
            "SHOP-7",
            &run(at("2026-11-09T09:00"), 40),
        ),
        session(
            "claude",
            "shop-2",
            "/w/shop",
            "SHOP-7",
            &run(at("2026-11-10T10:00"), 20),
        ),
        session(
            "codex",
            "blog-1",
            "/w/blog",
            "",
            &run(at("2026-11-10T10:10"), 30),
        ),
        lab,
    ]
}

/// The dashboard tool over `workspace` and the fixture sessions, with the days of November 2026
/// counted in UTC and today the 11th.
pub fn render_tool(
    workspace: &std::sync::Arc<construct_agent::workspace::Workspace>,
) -> std::sync::Arc<dyn construct_agent::chat::tool::ChatTool> {
    use chrono::TimeZone;
    let calendar = construct_agent::chat::dashboard_view::Calendar::at(
        &chrono::Utc
            .with_ymd_and_hms(2026, 11, 11, 12, 0, 0)
            .unwrap(),
    );
    construct_agent::chat::dashboard_view::tool(
        construct_agent::dashboard_document::Dashboard::open(std::sync::Arc::clone(workspace)),
        std::sync::Arc::new(|| Ok(fixture())),
        calendar,
    )
}
