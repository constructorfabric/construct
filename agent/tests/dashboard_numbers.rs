//! The numbers of the dashboard, from sessions given in the test: no database, no socket.
//! `dashboard_numbers_store.rs` asks the same through the socket of a real agent.

#![cfg(unix)]
// Test code: a failed `unwrap` is a failed test.
#![allow(clippy::unwrap_used)]

use std::time::{Duration, Instant};

mod common;

use common::sessions::{at, fixture, run, session};
use construct_agent::activity::assign;
use construct_agent::activity::rules::{
    Day, Grouping, Kind, PARTS_PER_MINUTE, SessionMinutes, calculate, calculate_named,
};
use construct_agent::dashboard::default_activity;
use construct_agent::dashboard_numbers::{Spec, numbers, respond};
use serde_json::{Value, json};

/// Three UTC days of the week of 2026-11-09 (a Monday).
fn week() -> Vec<Day> {
    ["09", "10", "11"]
        .iter()
        .map(|d| Day {
            label: format!("2026-11-{d}"),
            start: at(&format!("2026-11-{d}T00:00")),
            end: at(&format!("2026-11-{d}T00:00")) + 24 * 60,
        })
        .collect()
}

fn days_json() -> Value {
    json!([
        { "label": "2026-11-09", "start": "2026-11-09T00:00:00Z", "end": "2026-11-10T00:00:00Z" },
        { "label": "2026-11-10", "start": "2026-11-10T00:00:00Z", "end": "2026-11-11T00:00:00Z" },
        { "label": "2026-11-11", "start": "2026-11-11T00:00:00Z", "end": "2026-11-12T00:00:00Z" },
    ])
}

/// A request for `widget`, with the days of [`week`].
fn widget_request(widget: &str) -> String {
    json!({ "widget": widget, "days": days_json() }).to_string()
}

fn ask(document: &Value, request: &str, sessions: &[SessionMinutes]) -> Value {
    serde_json::from_str(&respond(request, Some(document), || {
        Ok(sessions.to_vec().into())
    }))
    .unwrap()
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn parts_of(minutes: f64) -> u64 {
    (minutes * PARTS_PER_MINUTE as f64).round() as u64
}

fn parts(minutes: &Value) -> u64 {
    parts_of(minutes.as_f64().unwrap())
}

#[test]
fn the_numbers_of_a_widget_equal_those_of_the_time_rules_for_the_same_input() {
    let sessions = fixture();
    let document = default_activity();
    let expected = |kind, grouping| calculate(&sessions, &week(), kind, grouping).unwrap();

    let table = ask(&document, &widget_request("by-project"), &sessions);
    let by_project = expected(Kind::Person, Grouping::Project);
    let groups = table["groups"].as_array().unwrap();
    assert_eq!(groups.len(), by_project.groups.len());
    for (got, want) in groups.iter().zip(&by_project.groups) {
        assert_eq!(got["key"], want.key.as_str());
        assert_eq!(parts(&got["minutes"]), want.parts);
        assert!((got["share"].as_f64().unwrap() - want.share).abs() < 1e-12);
        assert_eq!(got["session_count"], want.sessions);
    }
    assert_eq!(parts(&table["total_minutes"]), by_project.total_parts);
    assert_eq!(table["session_count"], by_project.sessions);
    assert_eq!(
        table["widget"],
        json!({ "id": "by-project", "kind": "table" })
    );

    let tickets = ask(&document, &widget_request("by-ticket"), &sessions);
    let by_ticket = expected(Kind::Person, Grouping::Ticket);
    let keys: Vec<_> = tickets["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["key"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        keys,
        by_ticket
            .groups
            .iter()
            .map(|g| g.key.clone())
            .collect::<Vec<_>>()
    );
    assert!(keys.contains(&assign::NO_TICKET.to_owned()));

    // The minutes of each day come with every answer, and equal the rules grouped by day.
    let bars = ask(&document, &widget_request("per-day"), &sessions);
    let by_day = expected(Kind::Person, Grouping::Day);
    let days = bars["days"].as_array().unwrap();
    assert_eq!(days.len(), 3);
    for (got, want) in days.iter().zip(&by_day.groups) {
        assert_eq!(got["label"], want.key.as_str());
        assert_eq!(parts(&got["minutes"]), want.parts);
    }
    assert_eq!(bars["groups"], json!([]));
    assert_eq!(parts(&bars["total_minutes"]), by_day.total_parts);
}

#[test]
fn a_period_with_a_kind_a_filter_and_a_grouping_is_asked_without_a_widget() {
    let sessions = fixture();
    let document = default_activity();
    let request = json!({
        "sessions": "all",
        "group_by": "tool",
        "filter": { "projects": ["blog", "lab"] },
        "days": days_json(),
    })
    .to_string();
    let answer = ask(&document, &request, &sessions);

    let kept: Vec<_> = sessions
        .iter()
        .filter(|s| s.project != "/w/shop")
        .cloned()
        .collect();
    let want = calculate(&kept, &week(), Kind::Both, Grouping::Tool).unwrap();
    assert_eq!(parts(&answer["total_minutes"]), want.total_parts);
    assert_eq!(
        answer["groups"].as_array().unwrap().len(),
        want.groups.len()
    );
    assert!(answer.get("widget").is_none());
}

#[test]
fn a_hidden_project_is_out_of_the_total_and_the_answer_counts_the_hidden() {
    let sessions = fixture();
    let mut document = default_activity();
    document["payload"]["hidden_projects"] = json!(["blog", "never-worked-on"]);

    let answer = ask(&document, &widget_request("total"), &sessions);
    let without: Vec<_> = sessions
        .iter()
        .filter(|s| s.project != "/w/blog")
        .cloned()
        .collect();
    let want = calculate(&without, &week(), Kind::Person, Grouping::Project).unwrap();
    assert_eq!(parts(&answer["total_minutes"]), want.total_parts);
    // Only a project that has time in the days counts as hidden.
    assert_eq!(answer["hidden_project_count"], 1);

    let table = ask(&document, &widget_request("by-project"), &sessions);
    let keys: Vec<_> = table["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["shop"]);

    let shown = ask(&default_activity(), &widget_request("total"), &sessions);
    assert_eq!(shown["hidden_project_count"], 0);
}

#[test]
fn a_hidden_project_with_time_only_outside_the_days_is_not_counted() {
    let sessions = vec![session(
        "claude",
        "old",
        "/w/old",
        "",
        &run(at("2026-10-01T09:00"), 10),
    )];
    let mut document = default_activity();
    document["payload"]["hidden_projects"] = json!(["old"]);
    let answer = ask(&document, &widget_request("total"), &sessions);
    assert_eq!(answer["hidden_project_count"], 0);
}

#[test]
fn a_rename_merges_two_rows_of_one_name() {
    let sessions = fixture();
    let mut document = default_activity();
    document["payload"]["project_names"] = json!({ "shop": "Store", "blog": "Store" });

    let table = ask(&document, &widget_request("by-project"), &sessions);
    let groups = table["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["key"], "Store");
    assert_eq!(groups[0]["session_count"], 3);

    // Both projects are one group, so the minute they share on the 10th is counted once, by
    // the rules: the same as the total of the unrenamed report.
    let plain = ask(
        &default_activity(),
        &widget_request("by-project"),
        &sessions,
    );
    assert_eq!(table["total_minutes"], plain["total_minutes"]);
    assert_eq!(parts(&groups[0]["minutes"]), parts(&table["total_minutes"]));
}

#[test]
fn a_name_the_page_gives_a_project_shows_in_its_row() {
    let sessions = fixture();
    let mut document = default_activity();
    document["payload"]["project_names"] = json!({ "/w/shop": "The shop" });
    let table = ask(&document, &widget_request("by-project"), &sessions);
    let keys: Vec<_> = table["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["key"].as_str().unwrap())
        .collect();
    assert!(keys.contains(&"The shop"));
    assert!(!keys.contains(&"shop"));
}

#[test]
fn a_widget_of_its_own_sessions_and_filter_counts_by_them() {
    let sessions = fixture();
    let mut document = default_activity();
    document["payload"]["widgets"][2]["sessions"] = json!("unattended");
    document["payload"]["widgets"][2]["filter"] = json!({ "tools": ["claude"] });
    let table = ask(&document, &widget_request("by-project"), &sessions);
    let kept: Vec<_> = sessions
        .iter()
        .filter(|s| s.tool == "claude")
        .cloned()
        .collect();
    let want = calculate(&kept, &week(), Kind::Unattended, Grouping::Project).unwrap();
    assert_eq!(parts(&table["total_minutes"]), want.total_parts);
    assert_eq!(table["groups"][0]["key"], "lab");
}

#[test]
fn an_unknown_widget_is_an_error_with_a_plain_reason() {
    let reply = ask(&default_activity(), &widget_request("nope"), &fixture());
    assert_eq!(
        reply,
        json!({ "error": "there is no widget \"nope\" in the dashboard" })
    );

    let long = "x".repeat(500);
    let reply = ask(&default_activity(), &widget_request(&long), &fixture());
    assert_eq!(
        reply,
        json!({ "error": "there is no such widget in the dashboard" })
    );
}

#[test]
fn a_widget_asked_without_a_good_document_is_an_error() {
    let reply: Value = serde_json::from_str(&respond(&widget_request("total"), None, || {
        Ok(fixture().into())
    }))
    .unwrap();
    assert_eq!(
        reply,
        json!({ "error": "the dashboard document is not available" })
    );
}

#[test]
fn a_request_that_cannot_be_used_says_why_and_repeats_nothing() {
    let document = default_activity();
    let day = |start: &str, end: &str| json!({ "label": "d", "start": start, "end": end });
    let cases = [
        ("not json secret-value".to_owned(), "the request is not a JSON object the command knows"),
        (
            json!({ "widget": "total", "days": [], "extra": "secret-value" }).to_string(),
            "the request is not a JSON object the command knows",
        ),
        (
            json!({ "widget": "total", "days": [] }).to_string(),
            "days is empty",
        ),
        (
            json!({ "widget": "total", "group_by": "project", "days": days_json() }).to_string(),
            "a request names a widget or the counting, not both",
        ),
        (
            json!({ "widget": "total", "days": [day("2026-11-09T00:00:30Z", "2026-11-10T00:00:00Z")] })
                .to_string(),
            "a day starts and ends on a whole minute in UTC, like 2026-11-11T00:00:00Z",
        ),
        (
            json!({ "widget": "total", "days": [day("2026-11-09T00:00:00+02:00", "2026-11-10T00:00:00Z")] })
                .to_string(),
            "a day starts and ends on a whole minute in UTC, like 2026-11-11T00:00:00Z",
        ),
        (
            json!({ "widget": "total", "days": [day("2026-11-10T00:00:00Z", "2026-11-09T00:00:00Z")] })
                .to_string(),
            "the day d ends before it starts",
        ),
        (
            json!({ "widget": "total", "days": (0..63).map(|n| json!({
                "label": format!("d{n}"),
                "start": "2026-11-09T00:00:00Z",
                "end": "2026-11-10T00:00:00Z",
            })).collect::<Vec<_>>() })
            .to_string(),
            "at most 62 days can be asked for at once",
        ),
    ];
    for (request, reason) in cases {
        let reply = respond(&request, Some(&document), || Ok(fixture().into()));
        assert_eq!(
            reply,
            json!({ "error": reason }).to_string(),
            "for {request}"
        );
        assert!(!reply.contains("secret-value"));
    }

    let overlap = json!({ "widget": "total", "days": [
        { "label": "a", "start": "2026-11-09T00:00:00Z", "end": "2026-11-10T12:00:00Z" },
        { "label": "b", "start": "2026-11-10T00:00:00Z", "end": "2026-11-11T00:00:00Z" },
    ] })
    .to_string();
    assert_eq!(
        respond(&overlap, Some(&document), || Ok(fixture().into())),
        json!({ "error": "the days a and b overlap" }).to_string()
    );
}

#[test]
fn the_store_not_being_ready_is_the_answer_and_nothing_is_read_for_a_bad_request() {
    let document = default_activity();
    let reply = respond(&widget_request("total"), Some(&document), || {
        Err("the activity time is being built")
    });
    assert_eq!(
        reply,
        json!({ "error": "the activity time is being built" }).to_string()
    );
    let reply = respond("{}", Some(&document), || {
        panic!("a bad request does not read the store")
    });
    assert!(reply.contains("error"));
}

#[test]
fn a_heavy_month_is_answered_within_a_second() {
    // A month of 31 days, 1,000 sessions of 150 active minutes each, in 40 projects, with a
    // year of older history behind them.
    let month_start = at("2026-11-01T00:00");
    let mut sessions: Vec<SessionMinutes> = (0..1000_i64)
        .map(|n| {
            let mut minutes = run(month_start + (n % 31) * 24 * 60 + (n % 7) * 60, 150);
            minutes.extend(run(month_start - 300_000 - n * 400, 100));
            let mut s = session(
                if n % 3 == 0 { "codex" } else { "claude" },
                &format!("s{n}"),
                &format!("/work/p{}/app", n % 40),
                &format!("T-{}", n % 90),
                &minutes,
            );
            s.unattended = n % 10 == 0;
            s
        })
        .collect();
    sessions.sort_by(|a, b| a.session.cmp(&b.session));
    let days: Vec<Value> = (0..31)
        .map(|d| {
            let start = month_start + d * 24 * 60;
            json!({
                "label": format!("day-{d:02}"),
                "start": format_time(start),
                "end": format_time(start + 24 * 60),
            })
        })
        .collect();
    let mut document = default_activity();
    document["payload"]["hidden_projects"] = json!(["p3/app"]);
    let request = json!({ "widget": "by-project", "days": days }).to_string();

    let started = Instant::now();
    let reply: Value = serde_json::from_str(&respond(&request, Some(&document), || {
        Ok(sessions.clone().into())
    }))
    .unwrap();
    let took = started.elapsed();
    assert!(took < Duration::from_secs(1), "took {took:?}");
    // 40 projects, minus the 4 that only scripts worked in (the user's own time is counted) and
    // the hidden one.
    assert_eq!(reply["groups"].as_array().unwrap().len(), 35);
    assert_eq!(reply["days"].as_array().unwrap().len(), 31);
    assert_eq!(reply["hidden_project_count"], 1);
    assert!(reply["total_minutes"].as_f64().unwrap() > 0.0);
}

fn format_time(minute: i64) -> String {
    let time = time::OffsetDateTime::from_unix_timestamp(minute * 60).unwrap();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:00Z",
        time.year(),
        u8::from(time.month()),
        time.day(),
        time.hour(),
        time.minute()
    )
}

#[test]
fn minutes_far_from_the_days_do_not_change_the_numbers() {
    let sessions = fixture();
    let near = numbers(&sessions, &week(), &Spec::default()).unwrap();
    let mut with_history = sessions.clone();
    for s in &mut with_history {
        s.minutes.extend(run(at("2026-01-05T09:00"), 90));
    }
    let far = numbers(&with_history, &week(), &Spec::default()).unwrap();
    assert_eq!(near, far);
}

fn keys(answer: &Value) -> Vec<&str> {
    answer["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["key"].as_str().unwrap())
        .collect()
}

fn total_parts(kept: &[SessionMinutes], kind: Kind) -> u64 {
    calculate(kept, &week(), kind, Grouping::Project)
        .unwrap()
        .total_parts
}

#[test]
fn the_sessions_of_the_page_count_for_a_widget_that_names_none() {
    let sessions = fixture();
    let mut document = default_activity();
    document["payload"]["sessions"] = json!("all");
    let answer = ask(&document, &widget_request("by-project"), &sessions);
    assert_eq!(
        parts(&answer["total_minutes"]),
        total_parts(&sessions, Kind::Both)
    );
    assert!(keys(&answer).contains(&"lab"));
}

#[test]
fn the_summary_of_the_day_and_the_bars_have_their_own_shape() {
    let sessions = fixture();
    let document = default_activity();
    let summary = ask(&document, &widget_request("end-of-day"), &sessions);
    assert_eq!(summary["widget"]["kind"], "day_summary");
    assert_eq!(
        keys(&summary),
        keys(&ask(&document, &widget_request("by-project"), &sessions))
    );
    let bars = ask(&document, &widget_request("per-day"), &sessions);
    assert_eq!(bars["widget"]["kind"], "bars_per_day");
    assert_eq!(bars["groups"], json!([]));
}

#[test]
fn a_filter_keeps_the_tickets_the_tools_and_the_names_it_lists() {
    let sessions = fixture();
    let document = default_activity();
    let ask_with = |filter: Value, group_by: &str| {
        let request = json!({ "filter": filter, "group_by": group_by, "days": days_json() });
        ask(&document, &request.to_string(), &sessions)
    };
    let only = |wanted: &dyn Fn(&SessionMinutes) -> bool| -> Vec<SessionMinutes> {
        sessions.iter().filter(|s| wanted(s)).cloned().collect()
    };

    let tickets = ask_with(json!({ "tickets": ["SHOP-7"] }), "ticket");
    assert_eq!(keys(&tickets), ["SHOP-7"]);
    assert_eq!(
        parts(&tickets["total_minutes"]),
        total_parts(&only(&|s| s.ticket == "SHOP-7"), Kind::Person)
    );
    let none = ask_with(json!({ "tickets": [assign::NO_TICKET] }), "ticket");
    assert_eq!(keys(&none), [assign::NO_TICKET]);
    assert_eq!(
        parts(&none["total_minutes"]),
        total_parts(&only(&|s| s.ticket.is_empty()), Kind::Person)
    );

    let tools = ask_with(json!({ "tools": ["codex"] }), "tool");
    assert_eq!(keys(&tools), ["codex"]);
    assert_eq!(
        parts(&tools["total_minutes"]),
        total_parts(&only(&|s| s.tool == "codex"), Kind::Person)
    );

    // A name the page gives a project is a name for a filter too.
    let mut renamed = default_activity();
    renamed["payload"]["project_names"] = json!({ "shop": "Store" });
    let request =
        json!({ "filter": { "projects": ["Store"] }, "group_by": "project", "days": days_json() });
    let answer = ask(&renamed, &request.to_string(), &sessions);
    assert_eq!(keys(&answer), ["Store"]);
}

#[test]
fn a_project_keeps_its_name_when_the_folder_of_the_same_name_is_hidden() {
    // Named among both folders they are `a/api` and `b/api`; the page's names do not move with
    // what is hidden, so the one that stays is still `a/api`.
    let day = at("2026-11-10T09:00");
    let sessions = vec![
        session("claude", "one", "/a/api", "", &run(day, 10)),
        session("claude", "two", "/b/api", "", &run(day + 60, 10)),
    ];
    let mut document = default_activity();
    document["payload"]["hidden_projects"] = json!(["b/api"]);
    let answer = ask(&document, &widget_request("by-project"), &sessions);
    assert_eq!(keys(&answer), ["a/api"]);
    assert_eq!(answer["hidden_project_count"], 1);
}

#[test]
fn the_hidden_count_is_of_folders_the_rules_give_time_in_the_days_each_on_its_own() {
    let day = at("2026-11-10T09:00");
    let mut script = session("claude", "script", "/w/script", "", &run(day, 5));
    script.unattended = true;
    let sessions = vec![
        // Both its active minutes are outside the day of three minutes below, and the pause
        // between them is one the rules count as work, so the day holds time of this folder.
        session(
            "claude",
            "bridge",
            "/w/bridge",
            "",
            &[at("2026-11-09T23:58"), at("2026-11-10T00:04")],
        ),
        // The script's minutes are all shared with the user's own `own`, which wins them: the
        // folder is counted all the same, for it is asked on its own.
        session("claude", "own", "/w/own", "", &run(day, 5)),
        script,
        // Two folders that the page shows under one name count as two.
        session("claude", "a", "/x/one", "", &run(day + 60, 5)),
        session("claude", "b", "/y/one", "", &run(day + 60, 5)),
        session("claude", "out", "/w/out", "", &run(day + 600, 5)),
    ];
    let mut document = default_activity();
    document["payload"]["project_names"] = json!({ "x/one": "One", "y/one": "One" });
    document["payload"]["hidden_projects"] = json!(["bridge", "script", "own", "One", "out"]);

    // The first day is the three minutes the bridge crosses; the second is the morning.
    let days = json!([
        { "label": "short", "start": "2026-11-10T00:00:00Z", "end": "2026-11-10T00:03:00Z" },
        { "label": "morning", "start": "2026-11-10T09:00:00Z", "end": "2026-11-10T11:00:00Z" },
    ]);
    let request = json!({ "widget": "total", "days": days }).to_string();
    let answer = ask(&document, &request, &sessions);
    // `bridge` and `script`, `own`, and the two folders named `One`; `out` has no time here.
    assert_eq!(answer["hidden_project_count"], 5);
    assert_eq!(answer["total_minutes"], 0.0);

    // Hiding one folder less changes the count by one, whatever else is hidden.
    document["payload"]["hidden_projects"] = json!(["script"]);
    let answer = ask(&document, &request, &sessions);
    assert_eq!(answer["hidden_project_count"], 1);
}

#[test]
fn a_block_that_leaves_the_days_through_any_pause_the_rules_allow_is_kept_whole() {
    // The mirror: one minute inside the last minutes of the day and one k minutes after its end.
    // The block is cut at the end of the day, so the minute after counts for nothing, and the
    // numbers still equal those of the rules on the whole input.
    let start = at("2026-11-10T00:00");
    let day = [Day {
        label: "d".to_owned(),
        start,
        end: start + 24 * 60,
    }];
    for k in 0..=9 {
        let sessions = vec![session(
            "claude",
            "s",
            "/w/a",
            "",
            &[start + 24 * 60 - 2, start + 24 * 60 + k],
        )];
        let got = numbers(&sessions, &day, &Spec::default()).unwrap();
        let names = assign::project_labels(["/w/a"]);
        let want =
            calculate_named(&sessions, &day, Kind::Person, Grouping::Project, &names).unwrap();
        assert_eq!(parts_of(got.total_minutes), want.total_parts, "k = {k}");
    }
}

#[test]
fn a_block_that_reaches_into_the_days_from_any_pause_the_rules_allow_is_kept_whole() {
    // Active k minutes before the day and one minute into it: for every k the pause rule
    // decides, and the numbers equal those of the rules on the whole input.
    let start = at("2026-11-10T00:00");
    let day = [Day {
        label: "d".to_owned(),
        start,
        end: start + 24 * 60,
    }];
    for k in 1..=9 {
        let sessions = vec![session("claude", "s", "/w/a", "", &[start - k, start + 1])];
        let spec = Spec::default();
        let got = numbers(&sessions, &day, &spec).unwrap();
        let names = assign::project_labels(["/w/a"]);
        let want =
            calculate_named(&sessions, &day, Kind::Person, Grouping::Project, &names).unwrap();
        assert_eq!(parts_of(got.total_minutes), want.total_parts, "k = {k}");
    }
}

#[test]
fn a_request_is_refused_while_the_settings_of_the_page_cannot_be_used() {
    let period = json!({ "group_by": "project", "days": days_json() }).to_string();
    let none = respond(&period, None, || Ok(fixture().into()));
    assert_eq!(
        none,
        json!({ "error": "the dashboard document is not available" }).to_string()
    );

    let mut broken = default_activity();
    broken["payload"]["hidden_projects"] = json!("blog");
    let reply = respond(&period, Some(&broken), || Ok(fixture().into()));
    assert_eq!(
        reply,
        json!({ "error": "the settings of the dashboard cannot be read" }).to_string()
    );
}

#[test]
fn the_days_are_checked_before_the_store_is_read() {
    let overlap = json!({ "widget": "total", "days": [
        { "label": "a", "start": "2026-11-09T00:00:00Z", "end": "2026-11-10T12:00:00Z" },
        { "label": "b", "start": "2026-11-10T00:00:00Z", "end": "2026-11-11T00:00:00Z" },
    ] })
    .to_string();
    let reply = respond(&overlap, Some(&default_activity()), || {
        panic!("a day set that cannot be used does not read the store")
    });
    assert_eq!(
        reply,
        json!({ "error": "the days a and b overlap" }).to_string()
    );
}

#[cfg(target_os = "linux")]
mod on_the_socket {
    use std::sync::Arc;
    use std::time::Duration;

    use construct_agent::activity::Feed;
    use construct_agent::dashboard::{ACTIVITY_FILE, default_activity};
    use construct_agent::dashboard_document::Dashboard;
    use construct_agent::dashboard_numbers::Numbers;
    use construct_agent::ipc::{Server, request};
    use construct_agent::workspace::Workspace;
    use serde_json::{Value, json};

    use super::days_json;

    /// The socket dispatch and the document hand-off, with an activity store that is not built:
    /// the answers that do not need the store.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_command_is_served_from_the_document_and_says_when_the_store_is_not_ready() {
        let root = std::env::temp_dir().join(format!("cb-numbers-socket-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let workspace = Arc::new(Workspace::open(&root.join("data")).unwrap());
        workspace
            .write(ACTIVITY_FILE, &default_activity().to_string())
            .unwrap();
        let dashboard = Dashboard::open(workspace);
        let feed = Feed::new();
        let server = Server::bind(&root.join("agent.sock"))
            .unwrap()
            .with_dashboard(dashboard.clone())
            .with_numbers(Numbers::new(
                feed,
                dashboard,
                tokio::runtime::Handle::current(),
            ));
        let socket = server.path().to_path_buf();
        std::thread::spawn(move || {
            let _ = server.serve();
        });
        let ask = move |argument: Value| -> Value {
            let line = format!("dashboard-numbers {argument}");
            let reply = request(&socket, &line, Duration::from_secs(10)).unwrap();
            serde_json::from_str(&reply).unwrap()
        };
        tokio::task::spawn_blocking(move || {
            assert_eq!(
                ask(json!({ "widget": "total", "days": days_json() })),
                json!({ "error": "the activity time is being built" })
            );
            assert_eq!(
                ask(json!({ "widget": "nope", "days": days_json() })),
                json!({ "error": "there is no widget \"nope\" in the dashboard" })
            );
        })
        .await
        .unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }
}
