//! The socket command `dashboard-numbers` on a real activity store, built from record files. The
//! test starts PostgreSQL, so it is ignored by default: `cargo test -- --ignored`.

#![cfg(target_os = "linux")]
// Test code: a failed `unwrap` is a failed test.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::pg_stand::PgStand;
use connectors::{Author, Record, Role, Turn};
use construct_agent::activity::Feed;
use construct_agent::dashboard::{ACTIVITY_FILE, default_activity};
use construct_agent::dashboard_details::Details;
use construct_agent::dashboard_document::Dashboard;
use construct_agent::dashboard_numbers::Numbers;
use construct_agent::ipc::{Server, request};
use construct_agent::records::{FileSink, Sink};
use construct_agent::workspace::Workspace;
use serde_json::{Value, json};

fn prompt(session: &str, time: &str, folder: &str, author: Author) -> Record {
    Record::Turn(Turn {
        tool: "claude".into(),
        session: session.into(),
        time: time.into(),
        folder: folder.into(),
        branch: "main".into(),
        role: Role::User,
        author,
        text: "words nobody may see".into(),
        tools: Vec::new(),
    })
}

/// A minute of Safari on a docs site, as the sampler keeps it.
fn write_screen(data: &Path) {
    let dir = data.join("records/screen");
    std::fs::create_dir_all(&dir).unwrap();
    let line = json!({
        "time": "2026-11-10T12:00:00Z",
        "until": "2026-11-10T12:00:55Z",
        "samples": 12,
        "idle_seconds": 0,
        "call": false,
        "locked": false,
        "app": { "name": "Safari", "bundle_id": "com.apple.Safari" },
        "window_title": "secret page",
        "tab": { "title": "secret page", "url": "https://docs.example.com/secret", "private": false },
    });
    std::fs::write(dir.join("2026-11-10.jsonl"), format!("{line}\n")).unwrap();
}

fn ask(socket: &Path, line: &str) -> Value {
    let reply = request(socket, line, Duration::from_secs(10)).unwrap();
    serde_json::from_str(&reply).unwrap()
}

/// Three sessions on one day: two of the user's own, one of a script.
fn write_records(data: &Path) {
    let mut sink = FileSink::new(data);
    for (session, folder, author, times) in [
        (
            "a",
            "/work/shop",
            Author::Person,
            [
                "2026-11-10T09:00:10Z",
                "2026-11-10T09:01:10Z",
                "2026-11-10T09:02:10Z",
            ],
        ),
        (
            "b",
            "/work/blog",
            Author::Person,
            [
                "2026-11-10T10:00:10Z",
                "2026-11-10T10:01:10Z",
                "2026-11-10T10:02:10Z",
            ],
        ),
        (
            "c",
            "/work/lab",
            Author::Unattended,
            [
                "2026-11-10T11:00:10Z",
                "2026-11-10T11:01:10Z",
                "2026-11-10T11:02:10Z",
            ],
        ),
    ] {
        for time in times {
            sink.append(&prompt(session, time, folder, author)).unwrap();
        }
    }
}

/// The numbers by site and the blocks of one site, of the time on screen the store holds (not of
/// the sessions): the answers of `dashboard-numbers` and `dashboard-sessions`.
fn screen_answers(socket: &Path) -> (Value, Value) {
    let days = json!([{ "label": "2026-11-10", "start": "2026-11-10T00:00:00Z", "end": "2026-11-11T00:00:00Z" }]);
    let request = |group: Option<&str>| {
        let mut request = json!({ "source": "on_screen", "group_by": "site", "days": days });
        if let Some(group) = group {
            request["group"] = json!(group);
        }
        request
    };
    (
        ask(socket, &format!("dashboard-numbers {}", request(None))),
        ask(
            socket,
            &format!("dashboard-sessions {}", request(Some("docs.example.com"))),
        ),
    )
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn the_socket_answers_the_numbers_of_a_widget_from_the_store() {
    let stand = PgStand::new("numbers-store").await;
    let data = stand.root.join("data/records");
    write_records(&data);
    write_screen(&data);

    let workspace = Arc::new(Workspace::open(&stand.root.join("workspace")).unwrap());
    let mut document = default_activity();
    document["payload"]["hidden_projects"] = json!(["blog"]);
    workspace
        .write(ACTIVITY_FILE, &document.to_string())
        .unwrap();
    let dashboard = Dashboard::open(Arc::clone(&workspace));

    let feed = Feed::new();
    let server = Server::bind(&stand.root.join("agent.sock"))
        .unwrap()
        .with_activity(feed.clone())
        .with_dashboard(dashboard.clone())
        .with_details(Details::new(
            feed.clone(),
            dashboard.clone(),
            None,
            tokio::runtime::Handle::current(),
        ))
        .with_numbers(Numbers::new(
            feed.clone(),
            dashboard,
            tokio::runtime::Handle::current(),
        ));
    let socket = server.path().to_path_buf();
    std::thread::spawn(move || {
        let _ = server.serve();
    });

    let by_project = json!({
        "widget": "by-project",
        "days": [{ "label": "2026-11-10", "start": "2026-11-10T00:00:00Z", "end": "2026-11-11T00:00:00Z" }],
    })
    .to_string();
    let line = format!("dashboard-numbers {by_project}");

    // Before the store is built the answer says so.
    let early = tokio::task::spawn_blocking({
        let (socket, line) = (socket.clone(), line.clone());
        move || ask(&socket, &line)
    })
    .await
    .unwrap();
    assert_eq!(
        early,
        json!({ "error": "the activity time is being built" })
    );

    let store = Arc::new(stand.store().await);
    let task = {
        let (feed, store, data) = (feed.clone(), store.clone(), data.clone());
        tokio::spawn(async move { feed.run(&store, &FileSink::new(&data)).await })
    };
    let start = Instant::now();
    let answer = loop {
        let (socket, line) = (socket.clone(), line.clone());
        let answer = tokio::task::spawn_blocking(move || ask(&socket, &line))
            .await
            .unwrap();
        if answer.get("error").is_none() {
            break answer;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "{answer}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };

    // The user's own time only (the script's session is not counted), the hidden project out.
    assert_eq!(
        answer["widget"],
        json!({ "id": "by-project", "kind": "table" })
    );
    assert_eq!(answer["total_minutes"], 3.0);
    assert_eq!(answer["hidden_project_count"], 1);
    assert_eq!(answer["groups"].as_array().unwrap().len(), 1);
    assert_eq!(answer["groups"][0]["key"], "shop");
    assert_eq!(answer["groups"][0]["share"], 1.0);
    assert_eq!(answer["days"][0]["minutes"], 3.0);
    assert!(!answer.to_string().contains("nobody may see"));

    let unknown = ask(
        &socket,
        r#"dashboard-numbers {"widget":"none","days":[{"label":"d","start":"2026-11-10T00:00:00Z","end":"2026-11-11T00:00:00Z"}]}"#,
    );
    assert_eq!(
        unknown,
        json!({ "error": "there is no widget \"none\" in the dashboard" })
    );

    let screen = tokio::task::spawn_blocking({
        let socket = socket.clone();
        move || screen_answers(&socket)
    })
    .await
    .unwrap();
    assert_eq!(screen.0["source"], "on_screen");
    assert_eq!(screen.0["total_minutes"], 1.0);
    assert_eq!(screen.0["groups"][0]["key"], "docs.example.com");
    assert_eq!(screen.1["blocks"][0]["seconds"], 60);
    assert_eq!(screen.1["blocks"][0]["app"], "Safari");
    assert!(!screen.0.to_string().contains("secret page"));
    assert!(!screen.1.to_string().contains("secret page"));

    task.abort();
    let _ = task.await;
    store.close().await;
    stand.finish().await;
}
