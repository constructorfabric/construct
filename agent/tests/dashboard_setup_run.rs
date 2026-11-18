//! The whole dashboard setup, from the socket commands to the numbers: made-up histories of three
//! repositories of two trackers are in the record files, the model is a scripted fake provider that
//! reads the session files and writes the findings, the report and the dashboard, and the activity
//! store is the real one over PostgreSQL. The test starts PostgreSQL, so it is ignored by default:
//! `cargo test -- --ignored`.

#![cfg(target_os = "linux")]
// Test code: a failed `unwrap` is a failed test.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::pg_stand::PgStand;
use common::scripted::Turn;
use common::setup_rig::{REPORT, Rig, SECRET, findings, minute, write};
use construct_agent::activity::Feed;
use construct_agent::dashboard::{ACTIVITY_FILE, default_activity};
use construct_agent::dashboard_details::Details;
use construct_agent::dashboard_document::Dashboard;
use construct_agent::dashboard_setup::Step;
use construct_agent::ipc::{Server, request};
use construct_agent::records::FileSink;
use serde_json::{Value, json};

/// The model's run: it reads the session files and the repository list, writes the three files,
/// looks at the page and ends.
fn script(shop: &Path) -> Vec<Turn> {
    let mut page = default_activity();
    page["payload"]["widgets"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": "by-group", "kind": "table", "group_by": "collection",
            "position": { "column": 0, "row": 14, "width": 12, "height": 4 },
        }));
    page["payload"]["project_names"] = json!({ shop.to_string_lossy(): "PLAT" });
    vec![
        Turn::Calls(vec![
            ("Glob", json!({ "pattern": "history/*" })),
            (
                "Read",
                json!({ "file_path": "history/0001-claude-code-shopsess-1.txt" }),
            ),
            (
                "Read",
                json!({ "file_path": "history/0002-claude-code-labsess-01.txt" }),
            ),
            (
                "Read",
                json!({ "file_path": "history/0003-codex-blogsess-1.txt" }),
            ),
            ("Read", json!({ "file_path": "history/folders.json" })),
        ]),
        Turn::Calls(vec![
            write("findings.json", &findings().to_string()),
            write("setup-report.md", REPORT),
            write(ACTIVITY_FILE, &page.to_string()),
        ]),
        Turn::Calls(vec![("render_dashboard", json!({}))]),
        Turn::Text("The dashboard is set up."),
    ]
}

async fn call(socket: &Path, line: String) -> Value {
    let socket = socket.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let reply = request(&socket, &line, Duration::from_secs(10)).unwrap();
        serde_json::from_str(&reply).unwrap()
    })
    .await
    .unwrap()
}

/// Asks until `done` likes the answer, a minute at most.
async fn until(socket: &Path, line: &str, what: &str, done: impl Fn(&Value) -> bool) -> Value {
    until_within(socket, Duration::from_secs(60), line, what, done).await
}

/// [`until`] with a limit of your own.
async fn until_within(
    socket: &Path,
    limit: Duration,
    line: &str,
    what: &str,
    done: impl Fn(&Value) -> bool,
) -> Value {
    let start = Instant::now();
    loop {
        let answer = call(socket, line.to_owned()).await;
        if done(&answer) {
            return answer;
        }
        assert!(start.elapsed() < limit, "{what}: {answer}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The sessions (by the first characters of their ids) in a group of the dashboard.
async fn sessions_in(socket: &Path, group_by: &str, group: &str) -> Vec<String> {
    let days = json!([{ "label": "week", "start": minute(3 * 24 * 60), "end": minute(-24 * 60) }]);
    let page = call(
        socket,
        format!(
            "dashboard-sessions {}",
            json!({ "group_by": group_by, "group": group, "days": days })
        ),
    )
    .await;
    let mut found: Vec<String> = page["sessions"]
        .as_array()
        .unwrap_or_else(|| panic!("{page}"))
        .iter()
        .map(|s| s["session"].as_str().unwrap().to_owned())
        .collect();
    found.sort();
    found
}

#[tokio::test(flavor = "multi_thread")]
// One test of the whole run: its steps follow one another.
#[allow(clippy::too_many_lines)]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn the_whole_setup_through_the_socket_commands_groups_the_work_and_finds_each_sessions_ticket()
 {
    let pg = PgStand::new("setup-run").await;
    let shop = pg.root.join("repos").join("shop");
    let feed = Feed::new();
    // The feed looks at the findings by itself only once a minute: a change it reads sooner it
    // was told of.
    feed.set_findings_poll(Duration::from_secs(60));
    let rig = Rig::new(
        &pg.root,
        tokio::runtime::Handle::current(),
        script(&shop),
        Some(feed.clone()),
        |_| {},
        |_| None,
    );
    let (data, workspace, settings) = (
        rig.data.clone(),
        Arc::clone(&rig.workspace),
        rig.settings.clone(),
    );
    feed.watch_findings(Arc::clone(&workspace));
    let dashboard = Dashboard::open(Arc::clone(&workspace));
    dashboard.watch();
    let server = Server::bind(&pg.root.join("agent.sock"))
        .unwrap()
        .with_activity(feed.clone())
        .with_dashboard(dashboard.clone())
        .with_details(Details::new(
            feed.clone(),
            dashboard,
            Some(settings),
            tokio::runtime::Handle::current(),
        ))
        .with_setup(rig.job.clone());
    let socket = server.path().to_path_buf();
    std::thread::spawn(move || {
        let _ = server.serve();
    });
    let (hold, bodies, host) = (&rig.hold, &rig.bodies, &rig.host);

    // The activity store is built from the record files and kept up to date, as the agent does.
    let store = Arc::new(pg.store().await);
    let task = {
        let (feed, store, data) = (feed.clone(), store.clone(), data.clone());
        tokio::spawn(async move { feed.run(&store, &FileSink::new(&data)).await })
    };
    until(&socket, "dashboard-state", "the store", |s| {
        s["state"] == "ready"
    })
    .await;

    // Before the setup nothing tells the trackers apart: the lab is not in any group.
    assert_eq!(
        sessions_in(&socket, "collection", "work").await,
        Vec::<String>::new()
    );

    // The run starts with the files, and is held there to look at.
    hold.at(Step::WritingFiles);
    let started = call(&socket, "dashboard-setup-start".to_owned()).await;
    assert!(started.get("error").is_none(), "{started}");
    let writing = until(&socket, "dashboard-setup-status", "the files", |s| {
        s["state"] == "writing_files"
    })
    .await;
    assert!(writing.get("report").is_none());

    // Then the reading: the files are written, the model has been asked nothing yet.
    hold.at(Step::ReadingAndBuilding);
    let reading = until(&socket, "dashboard-setup-status", "the reading", |s| {
        s["state"] == "reading_and_building"
    })
    .await;
    assert_eq!(reading["sessions"], 3, "{reading}");
    assert!(reading["text_bytes"].as_u64().unwrap() > 0);
    assert_eq!(reading["sent"], 0);
    // A second start is refused while the run goes.
    let refused = call(&socket, "dashboard-setup-start".to_owned()).await;
    assert!(refused["error"].as_str().unwrap().contains("already going"));

    hold.release();
    let done = until(&socket, "dashboard-setup-status", "the end", |s| {
        s["state"] == "done"
    })
    .await;
    assert_eq!(done["report"], REPORT);
    assert!(done["sent"].as_u64().unwrap() > 0 && done["received"].as_u64().unwrap() > 0);
    assert_eq!(done["sessions"], 3);

    // The model read the session files: the first request after its glob holds them.
    let seen = bodies.lock().unwrap()[1].clone();
    assert!(
        seen.contains("PLAT-12") && seen.contains("BLOG-3"),
        "{seen}"
    );

    // The dashboard that came out groups the work: the table of the groupings is on the page.
    let page = until(&socket, "dashboard-get", "the page", |p| {
        p["document"]["payload"]["widgets"]
            .as_array()
            .is_some_and(|w| w.len() > 5)
    })
    .await;
    let widgets = page["document"]["payload"]["widgets"].as_array().unwrap();
    assert!(
        widgets
            .iter()
            .any(|w| w["kind"] == "table" && w["group_by"] == "collection"),
        "{page}"
    );

    // The activity time read the findings: each session is on the ticket of its own tracker.
    let work = until_within(
        &socket,
        Duration::from_secs(5),
        &format!(
            "dashboard-sessions {}",
            json!({ "group_by": "collection", "group": "work",
                "days": [{ "label": "week", "start": minute(3 * 24 * 60), "end": minute(-24 * 60) }] })
        ),
        "the groups",
        |s| s["total"] == 2,
    )
    .await;
    assert_eq!(work["total"], 2);
    assert_eq!(
        sessions_in(&socket, "collection", "work").await,
        ["blogsess", "shopsess"]
    );
    assert_eq!(
        sessions_in(&socket, "collection", "No group").await,
        ["labsess-"]
    );
    // The shop's branch names the blog's ticket, the blog's text names the shop's more often.
    assert_eq!(
        sessions_in(&socket, "ticket", "PLAT-12").await,
        ["shopsess"]
    );
    assert_eq!(sessions_in(&socket, "ticket", "BLOG-3").await, ["blogsess"]);

    // The findings were kept as facts.
    let said = host.messages().join("\n");
    assert!(
        said.contains("task tracker alpha") && said.contains("task tracker beta"),
        "{said}"
    );
    assert!(done["facts"]["stored"].as_u64().unwrap() >= 1, "{done}");

    // No answer of the job holds the text of a session.
    for answer in [&writing, &reading, &done, &work] {
        assert!(!answer.to_string().contains(SECRET), "{answer}");
    }
    assert!(!said.contains(SECRET));

    task.abort();
    let _ = task.await;
    store.close().await;
    pg.finish().await;
}
