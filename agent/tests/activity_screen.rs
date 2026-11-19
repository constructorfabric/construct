//! The time on screen in the activity store against the embedded PostgreSQL. These tests unpack
//! and start PostgreSQL, so they are ignored by default: `cargo test -- --ignored`.

#![cfg(target_os = "linux")]
// Test code: a failed `unwrap` is a failed test.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use connectors::{Author, Mark, Record, Role, Turn};
use construct_agent::activity::screen_time::Block;
use construct_agent::activity::{Feed, Readiness, Store};
use construct_agent::records::{FileSink, Sink};
use construct_agent::screen::{App, Line, Reading, Tab};

mod common;

use common::pg_stand::PgStand as Stand;

const TITLE: &str = "quarterly numbers window";
const TAB_TITLE: &str = "pull request tab";
const ADDRESS: &str = "https://git.example.com/org/construct/pull/7?token=hidden";

fn session_records() -> Vec<Record> {
    vec![
        Record::Turn(Turn {
            tool: "claude".into(),
            session: "a".into(),
            time: "2026-11-11T09:00:00Z".into(),
            folder: "/work/construct".into(),
            branch: "PRJ-5-feature".into(),
            role: Role::User,
            author: Author::Person,
            text: "hello".into(),
            tools: Vec::new(),
        }),
        Record::Mark(Mark {
            tool: "claude".into(),
            session: "b".into(),
            time: "2026-11-11T09:30:00Z".into(),
            folder: "/work/dashboard".into(),
            branch: "main".into(),
            tools: Vec::new(),
        }),
    ]
}

fn line(time: &str, until: &str, samples: u64, reading: Reading) -> Line {
    Line {
        time: time.into(),
        until: until.into(),
        samples,
        reading,
    }
}

fn browser() -> Reading {
    Reading {
        app: Some(App {
            name: "Safari".into(),
            bundle_id: "com.apple.Safari".into(),
        }),
        window_title: Some(TITLE.into()),
        tab: Some(Tab {
            title: TAB_TITLE.into(),
            url: ADDRESS.into(),
            private: false,
        }),
        ..Reading::default()
    }
}

fn terminal() -> Reading {
    Reading {
        app: Some(App {
            name: "Terminal".into(),
            bundle_id: "com.apple.Terminal".into(),
        }),
        window_title: Some("PRJ-8 dashboard".into()),
        ..Reading::default()
    }
}

fn away() -> Reading {
    Reading {
        idle_seconds: 600,
        ..terminal()
    }
}

/// The runs the day files hold at the end, and the states they went through while they grew.
fn steps() -> Vec<Line> {
    vec![
        line("2026-11-11T10:00:00Z", "2026-11-11T10:00:00Z", 1, browser()),
        line("2026-11-11T10:00:00Z", "2026-11-11T10:00:25Z", 6, browser()),
        line(
            "2026-11-11T10:00:00Z",
            "2026-11-11T10:00:55Z",
            12,
            browser(),
        ),
        line(
            "2026-11-11T10:01:00Z",
            "2026-11-11T10:01:20Z",
            5,
            terminal(),
        ),
        line("2026-11-11T10:02:00Z", "2026-11-11T10:03:00Z", 13, away()),
        // A run that overlaps the next one in time: it is cut where the next starts.
        line(
            "2026-11-11T10:05:00Z",
            "2026-11-11T10:05:55Z",
            12,
            terminal(),
        ),
        line("2026-11-11T10:05:30Z", "2026-11-11T10:05:40Z", 3, browser()),
    ]
}

/// The last state of each run: what the day files hold.
fn finals() -> Vec<Line> {
    let mut last: Vec<Line> = Vec::new();
    for step in steps() {
        match last.iter_mut().find(|l| l.time == step.time) {
            Some(known) => *known = step,
            None => last.push(step),
        }
    }
    last
}

/// A sink with the sessions and the day files of `lines`.
fn sink(stand: &Stand, lines: &[Line]) -> FileSink {
    let root = stand.root.join("full");
    let mut sink = FileSink::new(&root);
    for record in session_records() {
        sink.append(&record).unwrap();
    }
    let dir = root.join("records/screen");
    fs::create_dir_all(&dir).unwrap();
    let mut text = String::new();
    for l in lines {
        text.push_str(&serde_json::to_string(l).unwrap());
        text.push('\n');
    }
    fs::write(dir.join("2026-11-11.jsonl"), text).unwrap();
    sink
}

async fn blocks(store: &Store) -> Vec<Block> {
    store.screen_blocks().await.unwrap()
}

/// Clear the store, then give it the sessions and the runs one by one.
async fn step_by_step(stand: &Stand, store: &Store, states: &[Line]) {
    store
        .rebuild(&FileSink::new(&stand.root.join("empty")))
        .await
        .unwrap();
    for record in session_records() {
        store.apply(&record).await.unwrap();
    }
    for state in states {
        store.apply_screen(state).await.unwrap();
    }
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn the_blocks_carry_the_app_the_site_the_project_and_the_ticket() {
    let stand = Stand::new("scr-blocks").await;
    let store = stand.store().await;
    store.rebuild(&sink(&stand, &finals())).await.unwrap();
    let seen: Vec<(String, u64, String, String, String, String)> = blocks(&store)
        .await
        .into_iter()
        .map(|b| (b.start, b.seconds, b.app, b.site, b.project, b.ticket))
        .collect();
    let row = |start: &str, seconds, app: &str, site: &str, project: &str, ticket: &str| {
        (
            start.to_owned(),
            seconds,
            app.to_owned(),
            site.to_owned(),
            project.to_owned(),
            ticket.to_owned(),
        )
    };
    assert_eq!(
        seen,
        [
            // The address names one project (the other is a word of no title) and no ticket.
            row(
                "2026-11-11T10:00:00Z",
                60,
                "Safari",
                "git.example.com",
                "construct",
                ""
            ),
            row(
                "2026-11-11T10:01:00Z",
                25,
                "Terminal",
                "",
                "dashboard",
                "PRJ-8"
            ),
            // Away from the first sample: nothing, so no block at 10:02. The next run is cut
            // where the one that starts inside it begins.
            row(
                "2026-11-11T10:05:00Z",
                30,
                "Terminal",
                "",
                "dashboard",
                "PRJ-8"
            ),
            row(
                "2026-11-11T10:05:30Z",
                15,
                "Safari",
                "git.example.com",
                "construct",
                ""
            ),
        ]
    );
    store.close().await;
    stand.finish().await;
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn a_rebuild_equals_the_step_by_step_store() {
    let stand = Stand::new("scr-rebuild").await;
    let store = stand.store().await;
    store.rebuild(&sink(&stand, &finals())).await.unwrap();
    let rebuilt = (
        blocks(&store).await,
        store.sessions().await.unwrap(),
        store.minutes().await.unwrap(),
    );
    assert!(!rebuilt.0.is_empty());

    // Each run grows in steps, as the sampler writes it.
    let mut grown = steps();
    step_by_step(&stand, &store, &grown).await;
    let stepped = |store: &Store| {
        let store = store.clone();
        async move {
            (
                store.screen_blocks().await.unwrap(),
                store.sessions().await.unwrap(),
                store.minutes().await.unwrap(),
            )
        }
    };
    assert_eq!(stepped(&store).await, rebuilt);
    // The states in the other order, the longest first, give the same numbers.
    grown.reverse();
    step_by_step(&stand, &store, &grown).await;
    assert_eq!(stepped(&store).await, rebuilt);
    store.close().await;
    stand.finish().await;
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn the_same_run_twice_changes_nothing() {
    let stand = Stand::new("scr-twice").await;
    let store = stand.store().await;
    for record in session_records() {
        store.apply(&record).await.unwrap();
    }
    for state in finals() {
        store.apply_screen(&state).await.unwrap();
    }
    let once = blocks(&store).await;
    assert!(!once.is_empty());
    for state in finals() {
        store.apply_screen(&state).await.unwrap();
        store.apply_screen(&state).await.unwrap();
    }
    assert_eq!(blocks(&store).await, once);
    // A shorter, earlier state of a run that came late takes nothing away.
    for state in steps() {
        store.apply_screen(&state).await.unwrap();
    }
    assert_eq!(blocks(&store).await, once);
    store.close().await;
    stand.finish().await;
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn the_store_holds_no_title_and_no_address() {
    let stand = Stand::new("scr-private").await;
    let store = stand.store().await;
    store.rebuild(&sink(&stand, &finals())).await.unwrap();
    assert!(!blocks(&store).await.is_empty());
    let mut conn = stand.connect().await;
    let rows: Vec<(String,)> = sqlx::query_as(
        "select row(s.*)::text from activity.screen s
         union all select row(t.*)::text from activity.sessions t
         union all select row(m.*)::text from activity.mentions m",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    assert!(!rows.is_empty());
    let all: String = rows.into_iter().map(|r| r.0).collect::<Vec<_>>().join("\n");
    for leaked in [
        TITLE,
        TAB_TITLE,
        "/pull/7",
        "token",
        "hidden",
        "quarterly",
        "dashboard window",
    ] {
        assert!(!all.contains(leaked), "{leaked} is in the store");
    }
    // The tables of the store have no column for a title or an address.
    let columns: Vec<(String,)> = sqlx::query_as(
        "select column_name::text from information_schema.columns
         where table_schema = 'activity' and table_name = 'screen'
         order by ordinal_position",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    let names: Vec<String> = columns.into_iter().map(|c| c.0).collect();
    assert_eq!(
        names,
        ["start", "seconds", "app", "site", "project", "ticket"]
    );
    store.close().await;
    stand.finish().await;
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn a_rebuild_without_screen_files_clears_the_blocks() {
    let stand = Stand::new("scr-clear").await;
    let store = stand.store().await;
    store.rebuild(&sink(&stand, &finals())).await.unwrap();
    assert!(!blocks(&store).await.is_empty());
    store
        .rebuild(&FileSink::new(&stand.root.join("empty")))
        .await
        .unwrap();
    assert!(blocks(&store).await.is_empty());
    store.close().await;
    stand.finish().await;
}

/// Starts `feed` on `store` over the files under `sink_dir`.
fn run_feed(feed: &Feed, store: &Store, sink_dir: &std::path::Path) -> tokio::task::JoinHandle<()> {
    let (feed, store, dir) = (feed.clone(), store.clone(), sink_dir.to_owned());
    tokio::spawn(async move { feed.run(&store, &FileSink::new(&dir)).await })
}

async fn wait_for(what: &str, mut done: impl AsyncFnMut() -> bool) {
    let start = Instant::now();
    while !done().await {
        assert!(start.elapsed() < Duration::from_secs(30), "{what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn a_run_the_sampler_tells_the_feed_of_reaches_the_store() {
    let stand = Stand::new("scr-observer").await;
    let store = Arc::new(stand.store().await);
    let feed = Feed::new();
    let task = run_feed(&feed, &store, &stand.root.join("none"));
    wait_for("the feed was not ready", async || {
        feed.readiness() == Readiness::Ready
    })
    .await;
    let observer = feed.screen_observer();
    let first = line(
        "2026-11-11T10:00:00Z",
        "2026-11-11T10:00:05Z",
        2,
        terminal(),
    );
    observer(&first);
    wait_for("the run did not reach the store", async || {
        blocks(&store).await.iter().map(|b| b.seconds).eq([10])
    })
    .await;
    // The same run, grown: one block with the new time.
    observer(&line(
        "2026-11-11T10:00:00Z",
        "2026-11-11T10:00:25Z",
        6,
        terminal(),
    ));
    wait_for("the grown run did not reach the store", async || {
        blocks(&store).await.iter().map(|b| b.seconds).eq([30])
    })
    .await;
    task.abort();
    let _ = task.await;
    store.close().await;
    stand.finish().await;
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn a_run_stored_before_its_project_is_named_again_when_the_project_appears() {
    let stand = Stand::new("scr-late-project").await;
    let store = Arc::new(stand.store().await);
    // The day file holds a run that names `construct`; no session of that project exists yet.
    let dir = stand.root.join("late");
    let run = line(
        "2026-11-11T10:00:00Z",
        "2026-11-11T10:00:55Z",
        12,
        browser(),
    );
    let screen_dir = dir.join("records/screen");
    fs::create_dir_all(&screen_dir).unwrap();
    fs::write(
        screen_dir.join("2026-11-11.jsonl"),
        format!("{}\n", serde_json::to_string(&run).unwrap()),
    )
    .unwrap();
    let feed = Feed::new();
    let task = run_feed(&feed, &store, &dir);
    wait_for("the first build did not finish", async || {
        blocks(&store).await.len() == 1
    })
    .await;
    assert_eq!(blocks(&store).await[0].project, "");

    // The session comes: the store says the projects changed, the feed builds again, and the block
    // is named, as a rebuild of the same files names it.
    let mut sink = FileSink::new(&dir);
    let record = session_records().remove(0);
    sink.append(&record).unwrap();
    feed.push(&record);
    wait_for("the block was not named again", async || {
        blocks(&store).await[0].project == "construct"
    })
    .await;
    let stepped = blocks(&store).await;
    store.rebuild(&FileSink::new(&dir)).await.unwrap();
    assert_eq!(blocks(&store).await, stepped);
    task.abort();
    let _ = task.await;
    store.close().await;
    stand.finish().await;
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn applying_a_record_says_whether_the_projects_changed() {
    let stand = Stand::new("scr-changed").await;
    let store = stand.store().await;
    let records = session_records();
    // Without blocks there is nothing to name again, so a new project says nothing.
    assert!(!store.apply(&records[0]).await.unwrap());
    store
        .apply_screen(&line(
            "2026-11-11T10:00:00Z",
            "2026-11-11T10:00:55Z",
            12,
            terminal(),
        ))
        .await
        .unwrap();
    // With a block, a new project says so, and a record of a known project does not.
    assert!(store.apply(&records[1]).await.unwrap());
    assert!(!store.apply(&records[0]).await.unwrap());
    assert!(!store.apply(&records[1]).await.unwrap());
    store.close().await;
    stand.finish().await;
}
