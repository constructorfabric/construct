//! The activity store against the embedded PostgreSQL, on fixture records. These tests unpack
//! and start PostgreSQL, so they are ignored by default: `cargo test -- --ignored`.

#![cfg(target_os = "linux")]
// Test code: a failed `unwrap` is a failed test.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use connectors::{Author, Mark, Record, Role, ToolCall, Turn};
use construct_agent::activity::{DBNAME, LOCK_KEY, RULES_VERSION, Session, Store};
use construct_agent::database::{Database, Paths, running_postmaster};
use construct_agent::records::{FileSink, Sink};
use sqlx::Connection;

/// A scratch directory with a running PostgreSQL, stopped and removed at the end.
struct Stand {
    root: PathBuf,
    database: Database,
}

impl Stand {
    async fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("cb-{name}-{}", std::process::id()));
        let paths = Paths {
            data_dir: root.join("data/postgres"),
            install_dir: root.join("data/postgres-bin"),
            socket_dir: root.join("run/pg"),
        };
        let database = Database::start(paths).await.unwrap();
        database.ensure_database(DBNAME).await.unwrap();
        Self { root, database }
    }

    async fn store(&self) -> Store {
        Store::open(&self.database.paths().socket_dir)
            .await
            .unwrap()
    }

    /// An empty directory of records, which clears a store when it is rebuilt from it.
    fn empty_sink(&self) -> FileSink {
        FileSink::new(&self.root.join("empty"))
    }

    /// A plain connection to the activity database, for what the store does not offer.
    async fn connect(&self) -> sqlx::PgConnection {
        let options = Store::connect_options(&self.database.paths().socket_dir);
        sqlx::PgConnection::connect_with(&options).await.unwrap()
    }

    /// A store that holds the rebuilt fixture, with the sink it was built from.
    async fn with_fixture(&self) -> (Store, FileSink) {
        let store = self.store().await;
        let sink = sink_with(self, &fixture());
        store.rebuild(&sink).await.unwrap();
        (store, sink)
    }

    async fn finish(self) {
        self.database.stop().await.unwrap();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Drop for Stand {
    /// A failed test must not leave its PostgreSQL running.
    fn drop(&mut self) {
        if let Some(pid) = running_postmaster(&self.database.paths().data_dir) {
            let _ = std::process::Command::new("kill")
                .arg(pid.to_string())
                .status();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn turn(
    session: &str,
    time: &str,
    role: Role,
    author: Author,
    folder: &str,
    branch: &str,
) -> Record {
    Record::Turn(Turn {
        tool: "claude".into(),
        session: session.into(),
        time: time.into(),
        folder: folder.into(),
        branch: branch.into(),
        role,
        author,
        text: format!("secret words at {time}"),
        tools: Vec::new(),
    })
}

fn mark(session: &str, time: &str, folder: &str, branch: &str) -> Record {
    Record::Mark(Mark {
        tool: "claude".into(),
        session: session.into(),
        time: time.into(),
        folder: folder.into(),
        branch: branch.into(),
        tools: Vec::new(),
    })
}

/// Four sessions: prompts by a person, a script and an unmarked log, marks in the same minute
/// as a turn and in minutes of their own, and a branch that changes. Session `c` has records at
/// the same instant (one spelled with a fraction), which only the tie-breaks put in order. One
/// record has no usable time and one is a tool call: neither counts.
fn fixture() -> Vec<Record> {
    use Author::{Person, Unattended, Unknown};
    use Role::{Assistant, User};
    vec![
        turn("a", "2026-11-11T10:00:05Z", User, Person, "/work/x", "main"),
        turn(
            "a",
            "2026-11-11T10:00:09.5Z",
            Assistant,
            Unknown,
            "/work/x",
            "main",
        ),
        mark("a", "2026-11-11T10:00:30Z", "/work/x", "main"),
        mark("a", "2026-11-11T10:03:00Z", "/work/x", "feature"),
        turn(
            "a",
            "2026-11-11T10:04:10Z",
            User,
            Unattended,
            "/work/x",
            "feature",
        ),
        turn(
            "a",
            "2026-11-11T10:04:11Z",
            User,
            Unknown,
            "/work/x",
            "feature",
        ),
        turn("b", "2026-11-12T08:00:00Z", User, Person, "/work/y", ""),
        mark("b", "2026-11-12T08:01:00Z", "/work/y", ""),
        Record::ToolCall(ToolCall {
            tool: "claude".into(),
            session: "b".into(),
            time: "2026-11-12T08:02:00Z".into(),
            call_id: "c1".into(),
            name: "search".into(),
            arguments: "{}".into(),
        }),
        mark("c", "2026-11-13T09:00:05Z", "/work/z", "x"),
        turn("c", "2026-11-13T09:00:05Z", User, Person, "/work/z", "y"),
        turn(
            "c",
            "2026-11-13T09:00:05.000Z",
            User,
            Person,
            "/work/z",
            "z",
        ),
        mark("d", "yesterday", "/work/w", ""),
    ]
}

async fn stored_version(stand: &Stand) -> i32 {
    sqlx::query_scalar("select version from activity.rules")
        .fetch_one(&mut stand.connect().await)
        .await
        .unwrap()
}

fn sink_with(stand: &Stand, records: &[Record]) -> FileSink {
    let mut sink = FileSink::new(&stand.root.join("records-dir"));
    for record in records {
        sink.append(record).unwrap();
    }
    sink
}

async fn state(store: &Store) -> (Vec<Session>, Vec<(String, String, String)>) {
    (
        store.sessions().await.unwrap(),
        store.minutes().await.unwrap(),
    )
}

/// Clear the store, then feed it `records` one by one.
async fn step_by_step(stand: &Stand, store: &Store, records: &[Record]) {
    store.rebuild(&stand.empty_sink()).await.unwrap();
    assert!(state(store).await.0.is_empty(), "the store was not cleared");
    for record in records {
        store.apply(record).await.unwrap();
    }
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn a_rebuild_has_the_counts_the_rules_give() {
    let stand = Stand::new("act-rules").await;
    let (store, _) = stand.with_fixture().await;

    let sessions = store.sessions().await.unwrap();
    assert_eq!(sessions.len(), 3, "the record without a time is left out");
    let a = &sessions[0];
    assert_eq!((a.tool.as_str(), a.session.as_str()), ("claude", "a"));
    assert_eq!(
        (a.folder.as_str(), a.branch.as_str()),
        ("/work/x", "feature")
    );
    assert_eq!(a.first_time, "2026-11-11T10:00:05Z");
    assert_eq!(a.last_time, "2026-11-11T10:04:11Z");
    assert_eq!(a.turns, 4);
    assert_eq!(
        (a.person_prompts, a.unattended_prompts, a.unknown_prompts),
        (1, 1, 1)
    );
    // 10:00, 10:03 and 10:04.
    assert_eq!(a.active_minutes, 3);
    let b = &sessions[1];
    assert_eq!((b.turns, b.person_prompts, b.active_minutes), (1, 1, 2));
    assert_eq!(b.branch, "");
    // Equal instants: the smaller spelling is the first time, the larger the last, and the
    // latest record by branch wins among records with the same spelling.
    let c = &sessions[2];
    assert_eq!(c.first_time, "2026-11-13T09:00:05.000Z");
    assert_eq!(c.last_time, "2026-11-13T09:00:05Z");
    assert_eq!((c.branch.as_str(), c.turns, c.active_minutes), ("y", 2, 1));

    // Nothing of what was said is stored, in any table the schema has.
    let mut conn = stand.connect().await;
    let tables: Vec<String> = sqlx::query_scalar(
        "select table_name::text from information_schema.tables where table_schema = 'activity'",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    assert!(tables.len() >= 4, "{tables:?}");
    for table in tables {
        let rows: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "select t::text from activity.{table} t"
        )))
        .fetch_all(&mut conn)
        .await
        .unwrap();
        assert!(
            !rows.iter().any(|r| r.contains("secret")),
            "{table}: {rows:?}"
        );
    }
    drop(conn);
    store.close().await;
    stand.finish().await;
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn step_by_step_equals_a_rebuild_in_any_order() {
    let stand = Stand::new("act-order").await;
    let (store, _) = stand.with_fixture().await;
    let rebuilt = state(&store).await;

    let forward = fixture();
    let mut backward = fixture();
    backward.reverse();
    // A fixed shuffle: every third record first, then the rest.
    let mut mixed: Vec<Record> = fixture().into_iter().step_by(3).collect();
    mixed.extend(
        fixture()
            .into_iter()
            .enumerate()
            .filter(|(i, _)| i % 3 != 0)
            .map(|(_, r)| r),
    );
    for records in [forward, backward, mixed] {
        step_by_step(&stand, &store, &records).await;
        assert_eq!(state(&store).await, rebuilt);
    }
    store.close().await;
    stand.finish().await;
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn the_same_record_twice_changes_nothing() {
    let stand = Stand::new("act-twice").await;
    let store = stand.store().await;
    let records = fixture();
    for record in &records {
        store.apply(record).await.unwrap();
    }
    let once = state(&store).await;
    // Each record again at once, and the whole stream again later.
    for record in &records {
        store.apply(record).await.unwrap();
        store.apply(record).await.unwrap();
    }
    assert_eq!(state(&store).await, once);
    store.close().await;
    stand.finish().await;
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn a_new_rules_version_drops_the_store_and_a_rebuild_fills_it() {
    let stand = Stand::new("act-version").await;
    let (store, sink) = stand.with_fixture().await;
    let rebuilt = state(&store).await;
    assert_eq!(stored_version(&stand).await, RULES_VERSION);
    store.close().await;

    // The store of an older rules version, with a row that no rule makes.
    let mut conn = stand.connect().await;
    sqlx::query("update activity.rules set version = $1")
        .bind(RULES_VERSION - 1)
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query(
        "insert into activity.sessions (tool, session, folder, branch, first_time, last_time)
         values ('old', 'old', '', '', '', '')",
    )
    .execute(&mut conn)
    .await
    .unwrap();
    drop(conn);

    let store = stand.store().await;
    assert_eq!(stored_version(&stand).await, RULES_VERSION);
    assert!(store.sessions().await.unwrap().is_empty());
    store.rebuild(&sink).await.unwrap();
    assert_eq!(state(&store).await, rebuilt);
    store.close().await;
    stand.finish().await;
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn a_store_whose_schema_is_gone_is_made_again() {
    let stand = Stand::new("act-gone").await;
    let (store, _) = stand.with_fixture().await;
    store.close().await;
    let mut conn = stand.connect().await;
    sqlx::query("drop schema activity cascade")
        .execute(&mut conn)
        .await
        .unwrap();
    drop(conn);
    let store = stand.store().await;
    assert_eq!(stored_version(&stand).await, RULES_VERSION);
    assert!(store.sessions().await.unwrap().is_empty());
    store.close().await;
    stand.finish().await;
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn a_store_that_cannot_be_read_is_not_dropped() {
    let stand = Stand::new("act-keep").await;
    let (store, _) = stand.with_fixture().await;
    store.close().await;
    // The version column holds something that is no number: reading it fails.
    let mut conn = stand.connect().await;
    for statement in [
        "alter table activity.rules alter column version type text using version::text",
        "update activity.rules set version = 'x'",
    ] {
        sqlx::query(statement).execute(&mut conn).await.unwrap();
    }
    assert!(
        Store::open(&stand.database.paths().socket_dir)
            .await
            .is_err()
    );
    let kept: i64 = sqlx::query_scalar("select count(*) from activity.sessions")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(kept, 3);
    drop(conn);
    stand.finish().await;
}

/// Hold the store's lock on a plain connection, start `writer`, and check that it waits for the
/// lock and then does its work.
async fn writer_waits_for_the_lock<F, Fut>(name: &str, writer: F)
where
    F: FnOnce(Store, PathBuf) -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let stand = Stand::new(name).await;
    let store = stand.store().await;
    let mut conn = stand.connect().await;
    let mut held = sqlx::Connection::begin(&mut conn).await.unwrap();
    sqlx::query("select pg_advisory_xact_lock($1)")
        .bind(LOCK_KEY)
        .execute(&mut *held)
        .await
        .unwrap();

    let task = tokio::spawn(writer(store.clone(), stand.root.clone()));
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    assert!(!task.is_finished(), "the writer did not wait for the lock");
    assert!(store.sessions().await.unwrap().is_empty());

    held.commit().await.unwrap();
    task.await.unwrap();
    assert_eq!(store.sessions().await.unwrap().len(), 1);
    drop(conn);
    store.close().await;
    stand.finish().await;
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn apply_waits_while_another_holds_the_lock() {
    let record = fixture().remove(0);
    writer_waits_for_the_lock("act-lock-a", |store, _| async move {
        store.apply(&record).await.unwrap();
    })
    .await;
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn rebuild_waits_while_another_holds_the_lock() {
    writer_waits_for_the_lock("act-lock-r", |store, root| async move {
        let mut sink = FileSink::new(&root.join("records-dir"));
        sink.append(&fixture().remove(0)).unwrap();
        store.rebuild(&sink).await.unwrap();
    })
    .await;
}
