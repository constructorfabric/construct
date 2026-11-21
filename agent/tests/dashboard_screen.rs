//! The numbers, the blocks and the chat answer of the On screen source, from blocks given in the
//! test: no database, no socket.

#![cfg(unix)]
// Test code: a failed `unwrap` is a failed test.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::TimeZone;
use construct_agent::activity::rules::Day;
use construct_agent::activity::rules::{SessionMinutes, minute_number};
use construct_agent::activity::screen_time::Block;
use construct_agent::chat::dashboard_view::{self, Calendar};
use construct_agent::dashboard::{Source, default_activity};
use construct_agent::dashboard_details::{respond_sessions, respond_summary};
use construct_agent::dashboard_document::Dashboard;
use construct_agent::dashboard_numbers::{Data, Spec, count, respond};
use construct_agent::dashboard_screen::ScreenGrouping;
use construct_agent::workspace::Workspace;
use serde_json::{Value, json};

fn block(start: &str, seconds: u64, app: &str, site: &str, project: &str, ticket: &str) -> Block {
    Block {
        start: start.to_owned(),
        seconds,
        app: app.to_owned(),
        site: site.to_owned(),
        project: project.to_owned(),
        ticket: ticket.to_owned(),
    }
}

fn folder(path: &str) -> SessionMinutes {
    SessionMinutes {
        tool: "claude".to_owned(),
        session: path.to_owned(),
        project: path.to_owned(),
        ticket: String::new(),
        unattended: false,
        area: String::new(),
        collection: String::new(),
        minutes: vec![minute_number("2026-11-10T09:00").unwrap()],
    }
}

/// Six blocks over the 9th to the 11th of November 2026 (UTC days). One crosses the midnight of
/// the 10th, one starts before the 9th, one is after the 11th, and some have no site, no project
/// or no ticket.
fn fixture() -> Data {
    Data {
        sessions: vec![folder("/w/shop"), folder("/w/blog")],
        blocks: vec![
            block("2026-11-08T23:55:00Z", 600, "Slack", "", "", ""),
            block("2026-11-09T09:00:00Z", 1800, "Code", "", "shop", "SHOP-7"),
            block("2026-11-09T09:30:00Z", 600, "Safari", "docs.rs", "", ""),
            block(
                "2026-11-10T23:50:00Z",
                1800,
                "Safari",
                "github.com",
                "blog",
                "BLOG-2",
            ),
            block("2026-11-11T10:00:00Z", 90, "Code", "", "shop", "SHOP-7"),
            block("2026-11-12T10:00:00Z", 600, "Code", "", "shop", "SHOP-7"),
        ]
        .into(),
    }
}

/// The seconds the fixture counts in the three days: the first block counts only its last 300,
/// the third one its 600 before and 1,200 after the midnight, the last one none.
const COUNTED_SECONDS: u64 = 300 + 1800 + 600 + 1800 + 90;

fn days_json() -> Value {
    json!([
        { "label": "2026-11-09", "start": "2026-11-09T00:00:00Z", "end": "2026-11-10T00:00:00Z" },
        { "label": "2026-11-10", "start": "2026-11-10T00:00:00Z", "end": "2026-11-11T00:00:00Z" },
        { "label": "2026-11-11", "start": "2026-11-11T00:00:00Z", "end": "2026-11-12T00:00:00Z" },
    ])
}

fn ask(document: &Value, request: &Value, data: &Data) -> Value {
    let reply = respond(&request.to_string(), Some(document), |_| Ok(data.clone()));
    serde_json::from_str(&reply).unwrap()
}

fn screen(group_by: Option<&str>) -> Value {
    let mut request = json!({ "source": "on_screen", "days": days_json() });
    if let Some(group_by) = group_by {
        request["group_by"] = json!(group_by);
    }
    request
}

fn same(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[allow(clippy::cast_precision_loss)]
fn minutes(seconds: u64) -> f64 {
    seconds as f64 / 60.0
}

fn groups(answer: &Value) -> Vec<(String, f64)> {
    answer["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            (
                g["key"].as_str().unwrap().to_owned(),
                g["minutes"].as_f64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn every_grouping_adds_up_to_the_total() {
    let data = fixture();
    let document = default_activity();
    for group_by in ["project", "ticket", "app", "site", "day"] {
        let answer = ask(&document, &screen(Some(group_by)), &data);
        let total = answer["total_minutes"].as_f64().unwrap();
        assert!(same(total, minutes(COUNTED_SECONDS)), "{group_by}: {total}");
        let sum: f64 = groups(&answer).iter().map(|(_, m)| m).sum();
        assert!(same(sum, total), "{group_by}: {sum} against {total}");
        let shares: f64 = answer["groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| g["share"].as_f64().unwrap())
            .sum();
        assert!(same(shares, 1.0), "{group_by}: shares {shares}");
        assert_eq!(answer["source"], "on_screen");
        assert_eq!(answer["session_count"], 5, "{group_by}");
    }
    let by = |group_by: &str| groups(&ask(&document, &screen(Some(group_by)), &data));
    let keyed = |pairs: &[(&str, u64)]| -> Vec<(String, f64)> {
        pairs
            .iter()
            .map(|(k, s)| ((*k).to_owned(), minutes(*s)))
            .collect()
    };
    assert_eq!(
        by("app"),
        keyed(&[("Safari", 2400), ("Code", 1890), ("Slack", 300)])
    );
    assert_eq!(
        by("site"),
        keyed(&[("No site", 2190), ("github.com", 1800), ("docs.rs", 600)])
    );
    assert_eq!(
        by("project"),
        keyed(&[("shop", 1890), ("blog", 1800), ("No project", 900)])
    );
    assert_eq!(
        by("ticket"),
        keyed(&[("SHOP-7", 1890), ("BLOG-2", 1800), ("No ticket", 900)])
    );
    // The days are in order, and the block across the midnight is split there.
    assert_eq!(
        by("day"),
        keyed(&[
            ("2026-11-09", 2700),
            ("2026-11-10", 600),
            ("2026-11-11", 1290)
        ])
    );
}

#[test]
fn the_total_and_the_bars_need_no_grouping() {
    let answer = ask(&default_activity(), &screen(None), &fixture());
    assert!(answer["groups"].as_array().unwrap().is_empty());
    let days: Vec<f64> = answer["days"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["minutes"].as_f64().unwrap())
        .collect();
    assert_eq!(days, [45.0, 10.0, 21.5]);
    // A day with no block is in the days and in a table by day, with no time.
    let mut empty = fixture();
    empty.blocks = Vec::new().into();
    let answer = ask(&default_activity(), &screen(Some("day")), &empty);
    assert!(same(answer["total_minutes"].as_f64().unwrap(), 0.0));
    assert_eq!(answer["groups"].as_array().unwrap().len(), 3);
}

#[test]
fn the_blocks_of_a_group_add_up_to_its_number() {
    let data = fixture();
    let document = default_activity();
    for group_by in ["project", "ticket", "app", "site", "day"] {
        let answer = ask(&document, &screen(Some(group_by)), &data);
        for group in answer["groups"].as_array().unwrap() {
            let mut request = screen(Some(group_by));
            request["group"] = group["key"].clone();
            let reply: Value = serde_json::from_str(&respond_sessions(
                &request.to_string(),
                Some(&document),
                |_| Ok(data.clone()),
            ))
            .unwrap();
            let blocks = reply["blocks"].as_array().unwrap();
            let seconds: u64 = blocks.iter().map(|b| b["seconds"].as_u64().unwrap()).sum();
            assert!(
                same(minutes(seconds), group["minutes"].as_f64().unwrap()),
                "{group_by} {}",
                group["key"]
            );
            assert_eq!(
                blocks.len() as u64,
                group["session_count"].as_u64().unwrap()
            );
            assert_eq!(reply["total"], group["session_count"]);
            assert_eq!(reply["source"], "on_screen");
        }
    }
}

#[test]
fn a_block_shows_its_times_app_site_project_and_ticket_and_nothing_else() {
    let mut request = screen(Some("site"));
    request["group"] = json!("github.com");
    request["utc_offset_minutes"] = json!(120);
    let reply: Value = serde_json::from_str(&respond_sessions(
        &request.to_string(),
        Some(&default_activity()),
        |_| Ok(fixture()),
    ))
    .unwrap();
    let row = &reply["blocks"][0];
    let keys: BTreeSet<&str> = row
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "start", "end", "seconds", "app", "site", "project", "ticket"
        ])
    );
    // The block crosses the midnight of the 10th; both days are in the group, so it is whole.
    assert_eq!(row["start"], "2026-11-11T01:50:00+02:00");
    assert_eq!(row["end"], "2026-11-11T02:20:00+02:00");
    assert_eq!(row["seconds"], 1800);
    assert_eq!(row["app"], "Safari");
    assert_eq!(row["project"], "blog");
    assert_eq!(row["ticket"], "BLOG-2");
    // The group of a day holds only the part of the block that is in the day.
    let mut request = screen(Some("day"));
    request["group"] = json!("2026-11-10");
    let reply: Value = serde_json::from_str(&respond_sessions(
        &request.to_string(),
        Some(&default_activity()),
        |_| Ok(fixture()),
    ))
    .unwrap();
    assert_eq!(reply["blocks"][0]["seconds"], 600);
    assert_eq!(reply["blocks"][0]["end"], "2026-11-11T00:00:00Z");
}

#[test]
fn a_document_with_no_source_reads_as_ai_sessions() {
    let mut data = fixture();
    data.sessions[0].minutes = (0..30)
        .map(|n| minute_number("2026-11-10T09:00").unwrap() + n)
        .collect();
    let document = default_activity();
    assert!(document["payload"].get("source").is_none());
    let widget = json!({ "widget": "by-project", "days": days_json() });
    let mut page = ask(&document, &widget, &data);
    // The 30 minutes of the session, not the time on screen.
    assert!(same(page["total_minutes"].as_f64().unwrap(), 30.0));
    assert!(page.get("source").is_none());
    assert_eq!(
        page.as_object_mut().unwrap().remove("widget").unwrap()["id"],
        "by-project"
    );
    let asked = |source: &str| {
        ask(
            &document,
            &json!({ "source": source, "group_by": "project", "days": days_json() }),
            &data,
        )
    };
    assert_eq!(asked("ai_sessions"), page);
    assert_ne!(asked("on_screen"), page);
    let plain = ask(
        &document,
        &json!({ "group_by": "project", "days": days_json() }),
        &data,
    );
    assert_eq!(plain, page);
}

#[test]
fn a_page_source_is_every_widgets_unless_the_widget_has_its_own() {
    let data = fixture();
    let mut document = default_activity();
    document["payload"]["source"] = json!("on_screen");
    // The widgets of AI sessions say so; the summary of the day has no other source.
    document["payload"]["widgets"][0]["source"] = json!("ai_sessions");
    document["payload"]["widgets"][4]["source"] = json!("ai_sessions");
    document["payload"]["widgets"][3] = json!({
        "id": "by-site", "kind": "table", "group_by": "site",
        "position": { "column": 6, "row": 6, "width": 6, "height": 5 },
    });
    construct_agent::dashboard::validate_activity(&document).unwrap();
    let on = |id: &str| {
        ask(
            &document,
            &json!({ "widget": id, "days": days_json() }),
            &data,
        )
    };
    assert!(same(
        on("per-day")["total_minutes"].as_f64().unwrap(),
        minutes(COUNTED_SECONDS)
    ));
    assert!(same(
        on("by-site")["total_minutes"].as_f64().unwrap(),
        minutes(COUNTED_SECONDS)
    ));
    // The widget that names AI sessions counts them: the sessions of the fixture have a minute.
    assert!(same(on("total")["total_minutes"].as_f64().unwrap(), 1.0));
}

#[test]
fn a_hidden_project_and_a_page_name_apply() {
    let data = fixture();
    let mut document = default_activity();
    document["payload"]["hidden_projects"] = json!(["shop"]);
    document["payload"]["project_names"] = json!({ "blog": "Journal" });
    let answer = ask(&document, &screen(Some("project")), &data);
    // The two blocks of the shop are left out of the total, and the name of the page shows.
    let counted = COUNTED_SECONDS - 1890;
    assert!(same(
        answer["total_minutes"].as_f64().unwrap(),
        minutes(counted)
    ));
    assert_eq!(
        groups(&answer),
        [
            ("Journal".to_owned(), minutes(1800)),
            ("No project".to_owned(), minutes(900))
        ]
    );
    assert_eq!(answer["hidden_project_count"], 1);
    // A project hidden by the name the page shows for it, and one that has no time in the days.
    let mut document = default_activity();
    document["payload"]["project_names"] = json!({ "shop": "Store" });
    document["payload"]["hidden_projects"] = json!(["Store", "never-worked-on"]);
    let answer = ask(&document, &screen(None), &data);
    assert!(same(
        answer["total_minutes"].as_f64().unwrap(),
        minutes(counted)
    ));
    assert_eq!(answer["hidden_project_count"], 1);
    // The blocks behind a group follow the same rules.
    let mut request = screen(Some("app"));
    request["group"] = json!("Code");
    let reply: Value = serde_json::from_str(&respond_sessions(
        &request.to_string(),
        Some(&document),
        |_| Ok(data.clone()),
    ))
    .unwrap();
    assert_eq!(reply["total"], 0);
}

#[test]
fn a_filter_keeps_the_projects_and_tickets_it_lists() {
    let data = fixture();
    let document = default_activity();
    let mut request = screen(Some("app"));
    request["filter"] = json!({ "tickets": ["No ticket"] });
    let answer = ask(&document, &request, &data);
    assert!(same(
        answer["total_minutes"].as_f64().unwrap(),
        minutes(900)
    ));
    request["filter"] = json!({ "projects": ["blog"], "tickets": ["BLOG-2"] });
    let answer = ask(&document, &request, &data);
    assert_eq!(groups(&answer), [("Safari".to_owned(), minutes(1800))]);
}

#[test]
fn what_the_source_cannot_answer_is_refused_in_plain_words() {
    let data = fixture();
    let document = default_activity();
    let refused = |request: &Value| {
        ask(&document, request, &data)["error"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let mut request = screen(Some("tool"));
    assert_eq!(
        refused(&request),
        "the time on screen is grouped by project, ticket, app, site or day"
    );
    request = screen(Some("app"));
    request["sessions"] = json!("all");
    assert_eq!(
        refused(&request),
        "sessions is of AI sessions: the time on screen has none"
    );
    request = screen(Some("app"));
    request["filter"] = json!({ "tools": ["claude"] });
    assert_eq!(
        refused(&request),
        "tools is of AI sessions: the time on screen has no tools"
    );
    let request = json!({ "group_by": "site", "days": days_json() });
    assert_eq!(
        refused(&request),
        "AI sessions are grouped by project, ticket, tool, collection or day"
    );
    let request = json!({ "widget": "total", "source": "on_screen", "days": days_json() });
    assert_eq!(
        refused(&request),
        "a request names a widget or the counting, not both"
    );
    // The summary of a day is of AI sessions.
    let summary = respond_summary(
        &json!({ "source": "on_screen", "day": days_json()[0] }).to_string(),
        Some(&document),
        |_| Ok(data.clone()),
    );
    assert_eq!(
        summary,
        json!({ "error": "the summary of a day is of AI sessions only" }).to_string()
    );
}

fn workspace(name: &str) -> Arc<Workspace> {
    let root = std::env::temp_dir().join(format!("cb-screen-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    Arc::new(Workspace::open(&root).unwrap())
}

/// `3 h 25 min`, `45 min`: whole minutes, as the tool and the page say them.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn spoken(minutes: f64) -> String {
    let whole = minutes.round() as u64;
    if whole >= 60 {
        format!("{} h {} min", whole / 60, whole % 60)
    } else {
        format!("{whole} min")
    }
}

#[test]
fn the_chat_tool_answers_as_the_page_for_the_same_request() {
    let data = fixture();
    let workspace = workspace("chat");
    let source = data.clone();
    let tool = dashboard_view::tool(
        Dashboard::open(Arc::clone(&workspace)),
        Arc::new(move |_| Ok(source.clone())),
        // Wednesday the 11th: this week runs from Monday the 9th to Sunday the 15th.
        Calendar::at(
            &chrono::Utc
                .with_ymd_and_hms(2026, 11, 11, 12, 0, 0)
                .unwrap(),
        ),
    );
    let week: Vec<Value> = (9..=15)
        .map(|d| {
            json!({
                "label": format!("2026-11-{d:02}"),
                "start": format!("2026-11-{d:02}T00:00:00Z"),
                "end": format!("2026-11-{:02}T00:00:00Z", d + 1),
            })
        })
        .collect();
    let document = default_activity();
    for group_by in ["project", "ticket", "app", "site", "day"] {
        let text = tool
            .run(
                &json!({ "period": "this_week", "source": "on_screen", "group_by": group_by })
                    .to_string(),
            )
            .unwrap();
        let page = ask(
            &document,
            &json!({ "source": "on_screen", "group_by": group_by, "days": week }),
            &data,
        );
        let mut lines = text.lines();
        assert_eq!(
            lines.next().unwrap(),
            format!("Table for this_week, on screen, by {group_by}")
        );
        assert_eq!(
            lines.next().unwrap(),
            format!(
                "  Total: {} ({} blocks)",
                spoken(page["total_minutes"].as_f64().unwrap()),
                page["session_count"]
            )
        );
        assert_eq!(
            lines.next().unwrap(),
            format!("  By {group_by}: time, share, blocks")
        );
        for group in page["groups"].as_array().unwrap() {
            assert_eq!(
                lines.next().unwrap(),
                format!(
                    "  {}: {}, {:.0}%, {}",
                    group["key"].as_str().unwrap(),
                    spoken(group["minutes"].as_f64().unwrap()),
                    group["share"].as_f64().unwrap() * 100.0,
                    group["session_count"]
                )
            );
        }
        assert_eq!(lines.next(), None, "{group_by}");
    }
    // What the source cannot answer is told to the model in the words of the page.
    for bad in [
        json!({ "period": "this_week", "source": "on_screen", "group_by": "tool" }),
        json!({ "period": "this_week", "source": "on_screen", "group_by": "app", "sessions": "all" }),
        json!({ "period": "this_week", "group_by": "app" }),
    ] {
        assert!(tool.run(&bad.to_string()).is_err(), "{bad}");
    }
}

#[test]
fn the_chat_tool_shows_the_widgets_of_the_time_on_screen_with_their_numbers() {
    let data = fixture();
    let workspace = workspace("widgets");
    let dashboard = Dashboard::open(Arc::clone(&workspace));
    let mut document = construct_agent::dashboard::default_activity_with_screen();
    document["payload"]["period"] = json!("this_week");
    let mut text = serde_json::to_string_pretty(&document).unwrap();
    text.push('\n');
    workspace
        .write(construct_agent::dashboard::ACTIVITY_FILE, &text)
        .unwrap();
    dashboard.settle();
    let tool = dashboard_view::tool(
        dashboard,
        Arc::new(move |_| Ok(data.clone())),
        Calendar::at(
            &chrono::Utc
                .with_ymd_and_hms(2026, 11, 11, 12, 0, 0)
                .unwrap(),
        ),
    );
    let shown = tool.run("").unwrap();
    assert!(
        shown.contains(
            "Widget screen-total (total, this_week, on screen)\n  Total: 1 h 27 min (6 blocks)"
        ),
        "{shown}"
    );
    assert!(shown.contains("Widget screen-by-app (table, this_week, on screen, by app)"));
    assert!(
        shown.contains("  By site: time, share, blocks\n  No site: 47 min, 54%, 4"),
        "{shown}"
    );
}

#[test]
fn a_request_for_a_month_answers_within_a_second() {
    // 31 days of 1,000 blocks of a minute each (a block per minute, 40 apps, 60 sites, 20
    // projects), after 200 days of older ones.
    let first = minute_number("2026-11-01T00:00").unwrap() * 60;
    let stamp = |second: i64| {
        let time = time::OffsetDateTime::from_unix_timestamp(second).unwrap();
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            time.year(),
            u8::from(time.month()),
            time.day(),
            time.hour(),
            time.minute(),
            time.second()
        )
    };
    let blocks_of_day = |day: i64| -> Vec<Block> {
        (0..1000_i64)
            .map(|n| {
                block(
                    &stamp(first + day * 86_400 + n * 60),
                    60,
                    &format!("app{}", n % 40),
                    &format!("site{}.test", n % 60),
                    &format!("p{}", n % 20),
                    &format!("T-{}", n % 90),
                )
            })
            .collect()
    };
    let mut blocks: Vec<Block> = (-200..31).flat_map(blocks_of_day).collect();
    blocks.sort_by(|a, b| a.start.cmp(&b.start));
    let data = Data {
        sessions: (0..40)
            .map(|n| folder(&format!("/work/p{}/app", n % 20)))
            .collect(),
        blocks: blocks.into(),
    };
    let days: Vec<Day> = (0..31)
        .map(|d| Day {
            label: format!("day-{d:02}"),
            start: first / 60 + d * 1440,
            end: first / 60 + (d + 1) * 1440,
        })
        .collect();
    let spec = Spec {
        source: Source::OnScreen,
        screen_grouping: Some(ScreenGrouping::Site),
        hidden_projects: vec!["p3".to_owned()],
        ..Spec::default()
    };
    // Only the count is timed, on data it borrows: reading and copying the store is not its work.
    // The bound is a smoke check of the whole counting (the selection, the split by day and the
    // groups) on a month and its history, which a pass over every block for every day would miss
    // by a wide margin.
    let started = Instant::now();
    let answer = count(&data, &days, &spec).unwrap();
    let took = started.elapsed();
    assert!(took < Duration::from_secs(1), "took {took:?}");
    // 60 sites, and the 3 of the hidden project are left out.
    assert_eq!(answer.groups.len(), 57);
    assert_eq!(answer.days.len(), 31);
    assert_eq!(answer.hidden_projects, 1);
    assert!(answer.total_minutes > 0.0);
}

#[test]
fn a_request_asks_for_the_data_of_its_source_only() {
    // What `read_data` then reads for each source (blocks only for On screen) is one match in the
    // store reader, and the store test below asks for On screen through it.
    let document = default_activity();
    let seen = std::cell::Cell::new(None);
    let load = |source| {
        seen.set(Some(source));
        Ok(fixture())
    };
    let _ = respond(
        &json!({ "group_by": "project", "days": days_json() }).to_string(),
        Some(&document),
        load,
    );
    assert_eq!(seen.get(), Some(Source::AiSessions));
    let _ = respond(&screen(Some("app")).to_string(), Some(&document), load);
    assert_eq!(seen.get(), Some(Source::OnScreen));
    // On a page that has widgets of both sources, a widget reads the data of its own.
    let document = construct_agent::dashboard::default_activity_with_screen();
    for (widget, source) in [
        ("total", Source::AiSessions),
        ("screen-total", Source::OnScreen),
    ] {
        seen.set(None);
        let _ = respond(
            &json!({ "widget": widget, "days": days_json() }).to_string(),
            Some(&document),
            load,
        );
        assert_eq!(seen.get(), Some(source), "{widget}");
    }
}
