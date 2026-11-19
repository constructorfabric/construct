//! The sessions behind a group, the summary of a day and the state of the data, from sessions
//! given in the test: no database, no socket. `dashboard_details_store.rs` asks the same through
//! the socket of a real agent.

#![cfg(unix)]
// Test code: a failed `unwrap` is a failed test.
#![allow(clippy::unwrap_used)]

use construct_agent::activity::Readiness;
use construct_agent::activity::rules::{SessionMinutes, minute_number};
use construct_agent::dashboard::default_activity;
use construct_agent::dashboard_details::{respond_sessions, respond_summary, state_json};
use construct_agent::dashboard_numbers::respond;
use serde_json::{Value, json};

fn at(time: &str) -> i64 {
    minute_number(time).unwrap()
}

fn session(tool: &str, name: &str, project: &str, ticket: &str, minutes: &[i64]) -> SessionMinutes {
    SessionMinutes {
        tool: tool.to_owned(),
        session: name.to_owned(),
        project: project.to_owned(),
        ticket: ticket.to_owned(),
        unattended: false,
        area: String::new(),
        collection: String::new(),
        minutes: minutes.to_vec(),
    }
}

/// `count` active minutes in a row from `start`.
fn run(start: &str, count: i64) -> Vec<i64> {
    (at(start)..at(start) + count).collect()
}

fn days() -> Value {
    json!([
        { "label": "2026-11-09", "start": "2026-11-09T00:00:00Z", "end": "2026-11-10T00:00:00Z" },
        { "label": "2026-11-10", "start": "2026-11-10T00:00:00Z", "end": "2026-11-11T00:00:00Z" },
        { "label": "2026-11-11", "start": "2026-11-11T00:00:00Z", "end": "2026-11-12T00:00:00Z" },
    ])
}

/// Sessions in three projects over three days. A script's session shares the 10th with the
/// user's, and a long session of the shop has no ticket on the 11th.
fn fixture() -> Vec<SessionMinutes> {
    let mut lab = session(
        "claude",
        "lab-session-0001",
        "/w/lab",
        "LAB-1",
        &run("2026-11-10T09:00", 30),
    );
    lab.unattended = true;
    vec![
        session(
            "claude",
            "shop-session-0001",
            "/w/shop",
            "SHOP-7",
            &run("2026-11-09T09:00", 40),
        ),
        session(
            "claude",
            "shop-session-0002",
            "/w/shop",
            "SHOP-7",
            &run("2026-11-10T10:00", 20),
        ),
        session(
            "codex",
            "blog-session-0001",
            "/w/blog",
            "",
            &run("2026-11-10T10:10", 30),
        ),
        session(
            "claude",
            "shop-session-0003",
            "/w/shop",
            "",
            &run("2026-11-11T08:00", 15),
        ),
        lab,
    ]
}

fn ask_sessions(request: &Value, sessions: &[SessionMinutes]) -> Value {
    let document = default_activity();
    let line = respond_sessions(&request.to_string(), Some(&document), || {
        Ok(sessions.to_vec())
    });
    serde_json::from_str(&line).unwrap()
}

fn ask_summary(request: &Value, sessions: &[SessionMinutes]) -> Value {
    let document = default_activity();
    let line = respond_summary(&request.to_string(), Some(&document), || {
        Ok(sessions.to_vec())
    });
    serde_json::from_str(&line).unwrap()
}

fn numbers(request: &Value, sessions: &[SessionMinutes]) -> Value {
    let document = default_activity();
    let line = respond(&request.to_string(), Some(&document), || {
        Ok(sessions.to_vec())
    });
    serde_json::from_str(&line).unwrap()
}

#[test]
fn the_sessions_of_a_group_add_up_to_the_group_in_the_numbers() {
    let sessions = fixture();
    for (group_by, sessions_kind) in [("project", "own"), ("ticket", "own"), ("project", "all")] {
        let request = json!({ "sessions": sessions_kind, "group_by": group_by, "days": days() });
        let table = numbers(&request, &sessions);
        let groups = table["groups"].as_array().unwrap();
        assert!(!groups.is_empty());
        let mut total = 0;
        for group in groups {
            let mut open = request.clone();
            open["group"] = group["key"].clone();
            let page = ask_sessions(&open, &sessions);
            assert_eq!(page["total"], group["session_count"], "{group_by} {group}");
            assert_eq!(
                json!(page["sessions"].as_array().unwrap().len()),
                group["session_count"]
            );
            total += page["total"].as_u64().unwrap();
        }
        // A session has one project and one ticket, so these groupings hold it in one group.
        assert_eq!(total, table["session_count"].as_u64().unwrap());
    }
}

#[test]
fn a_session_shows_its_time_in_the_users_offset_and_a_short_id_and_no_text() {
    let page = ask_sessions(
        &json!({
            "sessions": "own",
            "group_by": "ticket",
            "group": "SHOP-7",
            "utc_offset_minutes": 120,
            "days": days(),
        }),
        &fixture(),
    );
    assert_eq!(page["total"], 2);
    let first = &page["sessions"][0];
    assert_eq!(
        first,
        &json!({
            "start": "2026-11-09T11:00:00+02:00",
            "end": "2026-11-09T11:40:00+02:00",
            "minutes": 40,
            "tool": "claude",
            "project": "shop",
            "ticket": "SHOP-7",
            "session": "shop-ses",
        })
    );
    // The second session shared ten of its twenty minutes with another group, which takes half
    // of each: it holds fifteen, so the sessions add up to the row.
    assert_eq!(page["sessions"][1]["minutes"], 15);
    assert_eq!(page["next_offset"], Value::Null);
}

#[test]
fn the_next_page_follows_the_first() {
    let many: Vec<SessionMinutes> = (0..5)
        .map(|i| {
            session(
                "claude",
                &format!("session-{i}-xxxxxxxx"),
                "/w/shop",
                "SHOP-7",
                &run(&format!("2026-11-10T{:02}:00", 8 + i), 10),
            )
        })
        .collect();
    let request = |offset: usize| {
        json!({
            "sessions": "own",
            "group_by": "project",
            "group": "shop",
            "limit": 2,
            "offset": offset,
            "days": days(),
        })
    };
    let all = ask_sessions(
        &json!({ "sessions": "own", "group_by": "project", "group": "shop", "days": days() }),
        &many,
    );
    assert_eq!(all["total"], 5);

    let first = ask_sessions(&request(0), &many);
    assert_eq!(first["sessions"].as_array().unwrap().len(), 2);
    assert_eq!(first["next_offset"], 2);
    let second = ask_sessions(&request(2), &many);
    assert_eq!(second["next_offset"], 4);
    let third = ask_sessions(&request(4), &many);
    assert_eq!(third["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(third["next_offset"], Value::Null);

    let paged: Vec<&Value> = [&first, &second, &third]
        .iter()
        .flat_map(|page| page["sessions"].as_array().unwrap())
        .collect();
    let whole: Vec<&Value> = all["sessions"].as_array().unwrap().iter().collect();
    assert_eq!(paged, whole);
    let starts: Vec<&str> = whole.iter().map(|s| s["start"].as_str().unwrap()).collect();
    assert!(starts.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn a_request_that_cannot_be_answered_is_refused_in_plain_words() {
    let sessions = fixture();
    let refuse = |request: Value| ask_sessions(&request, &sessions)["error"].clone();
    assert_eq!(
        refuse(json!({ "sessions": "own", "days": days(), "group": "shop" })),
        "the request has nothing to group by, so it has no groups"
    );
    assert_eq!(
        refuse(json!({ "group_by": "project", "days": days() })),
        "group is the name of the group to open"
    );
    assert_eq!(
        refuse(json!({ "group_by": "project", "group": "x", "days": days(), "limit": 0 })),
        "limit is from 1 to 200"
    );
    assert_eq!(
        refuse(json!({ "group_by": "project", "group": "x", "days": days(), "offset": -1 })),
        "offset is not a position in the list"
    );
    assert_eq!(
        refuse(json!({
            "group_by": "project", "group": "x", "days": days(), "utc_offset_minutes": 5000
        })),
        "utc_offset_minutes is not an offset from UTC"
    );
    // A group nobody is in is an empty list, not an error.
    let none = ask_sessions(
        &json!({ "group_by": "project", "group": "nowhere", "days": days() }),
        &sessions,
    );
    assert_eq!(none["total"], 0);
}

#[test]
fn a_hidden_project_has_no_sessions_to_open() {
    let mut document = default_activity();
    document["payload"]["hidden_projects"] = json!(["blog"]);
    let line = respond_sessions(
        &json!({ "group_by": "project", "group": "blog", "days": days() }).to_string(),
        Some(&document),
        || Ok(fixture()),
    );
    let page: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(page["total"], 0);
}

#[test]
fn the_summary_lists_each_ticket_once_with_its_time_and_a_line_for_no_ticket() {
    let summary = ask_summary(
        &json!({
            "day": { "label": "2026-11-10", "start": "2026-11-10T00:00:00Z", "end": "2026-11-11T00:00:00Z" },
        }),
        &fixture(),
    );
    // The shop's ticket has twenty minutes of which ten are shared with the blog, which has none.
    assert_eq!(
        summary["lines"],
        json!(["No ticket (blog): 25 min", "SHOP-7: 15 min"])
    );
    assert_eq!(summary["day"], "2026-11-10");
    assert_eq!(summary["total_minutes"], 40.0);

    // Two sessions of one ticket are one line, and time with no ticket is a line of its project.
    let tickets = vec![
        session(
            "claude",
            "a",
            "/w/shop",
            "SHOP-7",
            &run("2026-11-10T09:00", 60),
        ),
        session(
            "claude",
            "b",
            "/w/blog",
            "SHOP-7",
            &run("2026-11-10T11:00", 60),
        ),
        session("claude", "c", "/w/shop", "", &run("2026-11-10T13:00", 30)),
        session("claude", "d", "/w/blog", "", &run("2026-11-10T14:00", 90)),
    ];
    let summary = ask_summary(
        &json!({
            "day": { "label": "2026-11-10", "start": "2026-11-10T00:00:00Z", "end": "2026-11-11T00:00:00Z" },
        }),
        &tickets,
    );
    assert_eq!(
        summary["lines"],
        json!([
            "SHOP-7: 2 h",
            "No ticket (blog): 1 h 30 min",
            "No ticket (shop): 30 min"
        ])
    );
}

#[test]
fn the_summary_of_a_widget_counts_its_sessions() {
    let document = default_activity();
    let summary: Value = serde_json::from_str(&respond_summary(
        &json!({
            "widget": "end-of-day",
            "day": { "label": "2026-11-10", "start": "2026-11-10T00:00:00Z", "end": "2026-11-11T00:00:00Z" },
        })
        .to_string(),
        Some(&document),
        || Ok(fixture()),
    ))
    .unwrap();
    assert_eq!(summary["total_minutes"], 40.0);
    let refused: Value = serde_json::from_str(&respond_summary(
        &json!({ "days": days() }).to_string(),
        Some(&document),
        || Ok(fixture()),
    ))
    .unwrap();
    assert_eq!(
        refused["error"],
        "the summary is of one day, in its own grouping"
    );
}

#[test]
fn the_state_is_reading_empty_or_ready_and_says_which_tool_is_off() {
    assert_eq!(
        state_json(Readiness::Reading { records_done: 1200 }, None, &[]),
        json!({ "state": "reading", "records_done": 1200, "tools_off": [] })
    );
    assert_eq!(
        state_json(Readiness::Ready, Some(true), &["codex"]),
        json!({ "state": "empty", "tools_off": ["codex"] })
    );
    assert_eq!(
        state_json(Readiness::Ready, Some(false), &["claude-code", "codex"]),
        json!({ "state": "ready", "tools_off": ["claude-code", "codex"] })
    );
    assert_eq!(
        state_json(Readiness::Unavailable, None, &[])["state"],
        "unavailable"
    );
}
