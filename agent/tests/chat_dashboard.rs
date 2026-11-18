//! The `render_dashboard` tool through the whole chat loop: a real chat with the file tools and the
//! dashboard tool over a real workspace folder, answering a scripted fake provider.
#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use construct_agent::activity::rules::SessionMinutes;
use construct_agent::chat::Chat;
use construct_agent::chat::dashboard_view::{self, Calendar};
use construct_agent::chat::files::file_tools;
use construct_agent::chat::tool::ChatTools;
use construct_agent::dashboard::{ACTIVITY_FILE, default_activity};
use construct_agent::dashboard_document::Dashboard;
use construct_agent::dashboard_numbers::respond;
use construct_agent::ipc::Server;
use construct_agent::settings::{Settings, Update};
use construct_agent::workspace::{DIR_NAME, Workspace};
use serde_json::{Value, json};
use time::OffsetDateTime;

mod common;

use common::scripted::{Turn, provider};
use common::sessions::{at, run, session};
use common::stand::{connect, deadlines};

static NEXT: AtomicU32 = AtomicU32::new(0);

const MESSAGE: &str = "what does my week look like";
const NOT_VALID: &str = "Error: the arguments are not valid for this tool";

struct Stand {
    base: PathBuf,
    workspace: PathBuf,
    socket: PathBuf,
    bodies: Arc<Mutex<Vec<String>>>,
    dashboard: Dashboard,
    _rt: tokio::runtime::Runtime,
}

/// Sessions in three projects over the 9th to the 11th of November 2026: `/w/shop` and `/w/blog`
/// overlap in time on the 10th, and `/w/lab` is a script's.
fn fixture() -> Vec<SessionMinutes> {
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

/// `count` days from `from` (a UTC date), as a window would send them: the labels are the dates and
/// the bounds are the UTC midnights.
fn days(from: &str, count: usize) -> Value {
    let first = at(&format!("{from}T00:00"));
    let stamp = |minute: i64| {
        let time = OffsetDateTime::from_unix_timestamp(minute * 60).unwrap();
        format!(
            "{:04}-{:02}-{:02}",
            time.year(),
            u8::from(time.month()),
            time.day()
        )
    };
    (0..count)
        .map(|n| {
            let start = first + 24 * 60 * i64::try_from(n).unwrap();
            json!({
                "label": stamp(start),
                "start": format!("{}T00:00:00Z", stamp(start)),
                "end": format!("{}T00:00:00Z", stamp(start + 24 * 60)),
            })
        })
        .collect()
}

/// The window of the tests: Wednesday the 11th of November, and the days from the first of October
/// to the end of November.
fn window() -> Value {
    json!({ "today": "2026-11-11", "days": days("2026-10-01", 61) })
}

impl Stand {
    fn new(turns: Vec<Turn>) -> Self {
        let base = std::env::temp_dir().join(format!(
            "cb-chat-dashboard-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let (data, socket) = (base.join("data"), base.join("run").join("agent.sock"));
        let (addr, bodies) = provider(turns);
        let settings = Settings::open(&base.join("config")).unwrap();
        let model = json!({ "model": {
            "base_url": format!("http://127.0.0.1:{}/v1", addr.port()), "model": "m", "api_key": "k" } });
        settings
            .apply(Update::parse(&model.to_string()).unwrap())
            .unwrap();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let workspace = Arc::new(Workspace::open(&data).unwrap());
        let dashboard = Dashboard::open(Arc::clone(&workspace));
        let calendar = Calendar::default();
        let mut tools = file_tools(&workspace);
        tools.push(dashboard_view::tool(
            dashboard.clone(),
            Arc::new(|| Ok(fixture())),
            calendar.clone(),
        ));
        let chat = Chat::new(settings.clone(), &data, rt.handle().clone(), deadlines())
            .unwrap()
            .with_calendar(calendar)
            .with_tools(ChatTools::new(tools).unwrap());
        let server = Server::bind(&socket)
            .unwrap()
            .with_settings(settings)
            .with_chat(chat);
        thread::spawn(move || server.serve());
        Self {
            workspace: data.join(DIR_NAME),
            base,
            socket,
            bodies,
            dashboard,
            _rt: rt,
        }
    }

    /// Sends the message with `window` and returns the events of the answer.
    fn say(&self, window: Option<Value>) -> Vec<Value> {
        let mut request = json!({ "message": MESSAGE });
        if let Some(window) = window {
            request["window"] = window;
        }
        connect(&self.socket, &format!("chat-send {request}"))
    }

    /// The results the model got in its last request, in order.
    fn results(&self) -> Vec<String> {
        let bodies = self.bodies.lock().unwrap();
        let last: Value = serde_json::from_str(bodies.last().unwrap()).unwrap();
        last["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["role"] == "tool")
            .map(|m| m["content"].as_str().unwrap().to_string())
            .collect()
    }

    fn lines(events: &[Value]) -> Vec<String> {
        events
            .iter()
            .filter(|e| e["event"] == "tool")
            .map(|e| e["line"].as_str().unwrap().to_string())
            .collect()
    }

    fn file(&self) -> String {
        std::fs::read_to_string(self.workspace.join(ACTIVITY_FILE)).unwrap()
    }

    /// Replaces the dashboard file and lets the watcher judge it.
    fn write(&self, text: &str) {
        std::fs::write(self.workspace.join(ACTIVITY_FILE), text).unwrap();
        self.dashboard.poll();
        self.dashboard.poll();
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn is_done(events: &[Value]) {
    assert_eq!(
        events.last().unwrap(),
        &json!({ "event": "done" }),
        "{events:?}"
    );
}

/// Seven days, Monday the 9th to Sunday the 15th: the week the page counts.
fn week() -> Value {
    days("2026-11-09", 7)
}

/// What the page's command answers for `request` over the fixture.
fn command(document: &Value, request: &Value) -> Value {
    serde_json::from_str(&respond(&request.to_string(), Some(document), || {
        Ok(fixture())
    }))
    .unwrap()
}

/// `3 h 25 min`, `45 min`: whole minutes, as the page shows time.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn time(minutes: &Value) -> String {
    let whole = minutes.as_f64().unwrap().round() as u64;
    if whole >= 60 {
        format!("{} h {} min", whole / 60, whole % 60)
    } else {
        format!("{whole} min")
    }
}

fn plural(count: u64) -> String {
    format!("{count} session{}", if count == 1 { "" } else { "s" })
}

/// The lines the tool writes for an answer of the command.
fn expected_lines(answer: &Value, bars: bool, by: &str) -> String {
    let mut text = format!(
        "  Total: {} ({})\n",
        time(&answer["total_minutes"]),
        plural(answer["session_count"].as_u64().unwrap())
    );
    if bars {
        for day in answer["days"].as_array().unwrap() {
            writeln!(
                text,
                "  {}: {}",
                day["label"].as_str().unwrap(),
                time(&day["minutes"])
            )
            .unwrap();
        }
    }
    let groups = answer["groups"].as_array().unwrap();
    if !groups.is_empty() {
        writeln!(text, "  By {by}: time, share, sessions").unwrap();
        for g in groups {
            writeln!(
                text,
                "  {}: {}, {:.0}%, {}",
                g["key"].as_str().unwrap(),
                time(&g["minutes"]),
                g["share"].as_f64().unwrap() * 100.0,
                g["session_count"]
            )
            .unwrap();
        }
    }
    let hidden = answer["hidden_project_count"].as_u64().unwrap();
    if hidden > 0 {
        writeln!(text, "  Hidden projects with time: {hidden}").unwrap();
    }
    text
}

fn tool_lists(stand: &Stand) -> Vec<Value> {
    stand
        .bodies
        .lock()
        .unwrap()
        .iter()
        .map(|b| serde_json::from_str::<Value>(b).unwrap()["tools"].clone())
        .collect()
}

#[test]
fn a_question_about_the_week_renders_the_numbers_of_the_pages_command() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![("render_dashboard", json!({}))]),
        Turn::Text("Here is your week."),
    ]);
    let document = default_activity();
    let events = stand.say(Some(window()));
    is_done(&events);
    assert_eq!(Stand::lines(&events), ["Looked at your dashboard"]);

    let mut expected = String::from("Dashboard \"Activity\"\n");
    for (id, kind, bars, by) in [
        ("total", "total", false, ""),
        ("per-day", "bars_per_day", true, ""),
        ("by-project", "table", false, "project"),
        ("by-ticket", "table", false, "ticket"),
        ("end-of-day", "day_summary", false, "project"),
    ] {
        let answer = command(&document, &json!({ "widget": id, "days": week() }));
        let by_text = if by.is_empty() {
            String::new()
        } else {
            format!(", by {by}")
        };
        writeln!(
            expected,
            "Widget {id} ({kind}, this_week, own sessions{by_text})"
        )
        .unwrap();
        expected.push_str(&expected_lines(&answer, bars, by));
    }
    let results = stand.results();
    assert_eq!(results, [expected.trim_end()]);
    // The fixture has time: the numbers are not all zero.
    assert!(
        results[0].contains("Total: 1 h 20 min (3 sessions)"),
        "{results:?}"
    );
    // No text of a message is in a result.
    assert!(!results[0].contains(MESSAGE));
}

#[test]
fn a_period_with_a_grouping_renders_that_table_and_leaves_the_file_alone() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![(
            "render_dashboard",
            json!({ "period": "this_week", "group_by": "project", "sessions": "all" }),
        )]),
        Turn::Calls(vec![(
            "render_dashboard",
            json!({ "period": "this_week", "group_by": "tool",
                    "filter": { "projects": ["/w/shop"] } }),
        )]),
        Turn::Text("Done."),
    ]);
    let before = stand.file();
    let document = default_activity();
    let events = stand.say(Some(window()));
    is_done(&events);
    assert_eq!(
        Stand::lines(&events),
        [
            "Looked at your dashboard: this_week, by project",
            "Looked at your dashboard: this_week, by tool"
        ]
    );
    let all = command(
        &document,
        &json!({ "days": week(), "group_by": "project", "sessions": "all" }),
    );
    let shop = command(
        &document,
        &json!({ "days": week(), "group_by": "tool", "filter": { "projects": ["/w/shop"] } }),
    );
    assert_eq!(
        stand.results(),
        [
            format!(
                "Table for this_week, all sessions, by project\n{}",
                expected_lines(&all, false, "project").trim_end()
            ),
            format!(
                "Table for this_week, own sessions, by tool\n{}",
                expected_lines(&shop, false, "tool").trim_end()
            ),
        ]
    );
    assert_eq!(stand.file(), before);
}

#[test]
fn a_document_with_errors_returns_them_with_their_places() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![("render_dashboard", json!({}))]),
        Turn::Text("It does not pass."),
    ]);
    let mut broken = default_activity();
    broken["payload"]["period"] = json!("next_year");
    stand.write(&broken.to_string());
    let events = stand.say(Some(window()));
    is_done(&events);
    assert_eq!(Stand::lines(&events), ["Checked your dashboard file"]);
    let results = stand.results();
    assert_eq!(results.len(), 1);
    assert!(
        results[0].starts_with(
            "The dashboard file has an error. The page keeps showing the last good version.\n- /payload/period: "
        ),
        "{results:?}"
    );
    assert!(!results[0].contains("next_year"));
}

#[test]
fn a_file_that_is_not_json_is_told_with_its_line_and_the_last_good_version_stays() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![("render_dashboard", json!({}))]),
        Turn::Text("It does not parse."),
    ]);
    stand.write("{\n  \"type\": ,\n}");
    let events = stand.say(Some(window()));
    is_done(&events);
    let results = stand.results();
    assert_eq!(
        results,
        [
            "The dashboard file has an error. The page keeps showing the last good version.\n- line 2, column 11: the file is not valid JSON"
        ]
    );
    // A good file again: the error goes.
    stand.write(&default_activity().to_string());
    assert!(stand.dashboard.to_json().get("error").is_none());
}

#[test]
fn a_bad_argument_is_a_result_with_a_plain_reason() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![
            ("render_dashboard", json!({ "period": "this_week" })),
            (
                "render_dashboard",
                json!({ "period": "next_week", "group_by": "tool" }),
            ),
            (
                "render_dashboard",
                json!({ "group_by": "tool", "extra": true }),
            ),
        ]),
        Turn::Text("I could not do that."),
    ]);
    is_done(&stand.say(Some(window())));
    let results = stand.results();
    assert_eq!(results.len(), 3);
    for result in &results {
        assert!(result.starts_with(NOT_VALID), "{result}");
        assert!(result.contains("period is one of"), "{result}");
        assert_eq!(result, &results[0]);
    }
}

#[test]
fn without_a_window_the_days_are_not_guessed() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![("render_dashboard", json!({}))]),
        Turn::Text("I cannot tell."),
    ]);
    is_done(&stand.say(None));
    let results = stand.results();
    assert!(
        results[0].starts_with("Error: the window has not told which days belong to which period"),
        "{results:?}"
    );
}

#[test]
fn a_window_that_is_not_a_calendar_is_refused() {
    let stand = Stand::new(vec![]);
    for window in [
        json!({ "today": "2026-11-11", "days": [] }),
        // Today is not one of the days.
        json!({ "today": "2026-12-25", "days": days("2026-11-09", 7) }),
        // A day that is not a whole minute in UTC.
        json!({ "today": "2026-11-09", "days": [
            { "label": "2026-11-09", "start": "2026-11-09T00:00:30Z", "end": "2026-11-10T00:00:00Z" }] }),
        // A gap between two days.
        json!({ "today": "2026-11-09", "days": [
            { "label": "2026-11-09", "start": "2026-11-09T00:00:00Z", "end": "2026-11-10T00:00:00Z" },
            { "label": "2026-11-11", "start": "2026-11-11T00:00:00Z", "end": "2026-11-12T00:00:00Z" }] }),
    ] {
        let events = stand.say(Some(window));
        assert_eq!(
            events,
            [json!({ "event": "error", "reason": "invalid request" })]
        );
    }
}

#[test]
fn the_tool_list_is_the_same_in_every_request_and_holds_the_tool_once() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![("render_dashboard", json!({}))]),
        Turn::Calls(vec![(
            "render_dashboard",
            json!({ "period": "last_week", "group_by": "day" }),
        )]),
        Turn::Text("Done."),
    ]);
    is_done(&stand.say(Some(window())));
    let lists = tool_lists(&stand);
    assert_eq!(lists.len(), 3);
    assert!(lists.iter().all(|list| list == &lists[0]));
    let names: Vec<&str> = lists[0]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Read", "Write", "Edit", "Glob", "render_dashboard"]);
    // Last week has no time in the fixture, and the table says so for every day.
    let results = stand.results();
    assert!(
        results[1].contains("Total: 0 min (0 sessions)"),
        "{results:?}"
    );
}
