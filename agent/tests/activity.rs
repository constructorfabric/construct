//! The activity store against the embedded PostgreSQL, on fixture records. These tests unpack
//! and start PostgreSQL, so they are ignored by default: `cargo test -- --ignored`.

#![cfg(target_os = "linux")]
// Test code: a failed `unwrap` is a failed test.
#![allow(clippy::unwrap_used)]

use connectors::{Author, Mark, Record, Role, ToolCall, Turn};
use construct_agent::activity::rules::{Day, Grouping, Kind, PARTS_PER_MINUTE, minute_number};
use construct_agent::activity::{LOCK_KEY, RULES_VERSION, Session, Store};
use construct_agent::records::{FileSink, Sink};
use std::path::PathBuf;

mod common;

use common::pg_stand::PgStand as Stand;

impl Stand {
    /// An empty directory of records, which clears a store when it is rebuilt from it.
    fn empty_sink(&self) -> FileSink {
        FileSink::new(&self.root.join("empty"))
    }

    /// A store that holds the rebuilt fixture, with the sink it was built from.
    async fn with_fixture(&self) -> (Store, FileSink) {
        let store = self.store().await;
        let sink = sink_with(self, &fixture());
        store.rebuild(&sink).await.unwrap();
        (store, sink)
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
    said(
        session,
        time,
        role,
        author,
        (folder, branch),
        &format!("secret words at {time}"),
    )
}

/// A turn with a text of the caller's.
fn said(
    session: &str,
    time: &str,
    role: Role,
    author: Author,
    (folder, branch): (&str, &str),
    text: &str,
) -> Record {
    Record::Turn(Turn {
        tool: "claude".into(),
        session: session.into(),
        time: time.into(),
        folder: folder.into(),
        branch: branch.into(),
        role,
        author,
        text: text.into(),
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
async fn the_stored_minutes_give_the_time_of_a_period() {
    let stand = Stand::new("act-period").await;
    let (store, _) = stand.with_fixture().await;
    let day = |date: &str, next: &str| Day {
        label: date.into(),
        start: minute_number(&format!("{date}T00:00")).unwrap(),
        end: minute_number(&format!("{next}T00:00")).unwrap(),
    };
    let days = [
        day("2026-11-11", "2026-11-12"),
        day("2026-11-12", "2026-11-13"),
        day("2026-11-13", "2026-11-14"),
    ];
    let whole = |minutes: u64| minutes * PARTS_PER_MINUTE;

    // Session `a` is active at 10:00, 10:03 and 10:04, so its block is 5 minutes; `b` has 2
    // and `c` has 1. A prompt of an unknown author counts as the user's own.
    let report = store
        .activity(&days, Kind::Both, Grouping::Day)
        .await
        .unwrap();
    let per_day: Vec<u64> = report.groups.iter().map(|g| g.parts).collect();
    assert_eq!(per_day, vec![whole(5), whole(2), whole(1)]);
    assert_eq!(report.total_parts, whole(8));
    assert_eq!(report.sessions, 3);
    let own = store
        .activity(&days, Kind::Person, Grouping::Project)
        .await
        .unwrap();
    assert_eq!(own.total_parts, whole(8));
    assert_eq!(own.groups[0].key, "x");
    let unattended = store
        .activity(&days, Kind::Unattended, Grouping::Tool)
        .await
        .unwrap();
    assert_eq!(unattended.total_parts, 0);
    store.close().await;
    stand.finish().await;
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn the_chat_of_the_app_is_not_stored_as_activity() {
    let stand = Stand::new("act-chat").await;
    let store = stand.store().await;
    let chat = |time: &str| {
        Record::Turn(Turn {
            tool: "chat".into(),
            session: "c".into(),
            time: time.into(),
            folder: String::new(),
            branch: String::new(),
            role: Role::User,
            author: Author::Unknown,
            text: "hello".into(),
            tools: Vec::new(),
        })
    };
    let work = turn(
        "w",
        "2026-11-11T10:00:00Z",
        Role::User,
        Author::Person,
        "/w/x",
        "main",
    );
    let sink = sink_with(&stand, &[work, chat("2026-11-11T11:00:00Z")]);
    let days = [Day {
        label: "2026-11-11".into(),
        start: minute_number("2026-11-11T00:00").unwrap(),
        end: minute_number("2026-11-12T00:00").unwrap(),
    }];
    let by_tool = |store: Store| {
        let days = days.clone();
        async move {
            store
                .activity(&days, Kind::Both, Grouping::Tool)
                .await
                .unwrap()
        }
    };

    // A rebuild leaves the chat out.
    store.rebuild(&sink).await.unwrap();
    let report = by_tool(store.clone()).await;
    let tools: Vec<&str> = report.groups.iter().map(|g| g.key.as_str()).collect();
    assert_eq!(tools, ["claude"]);
    let total = report.total_parts;

    // So does a record applied while the agent runs, and the time of the week does not change.
    store.apply(&chat("2026-11-11T12:00:00Z")).await.unwrap();
    let report = by_tool(store.clone()).await;
    assert_eq!(report.groups.len(), 1);
    assert_eq!(report.total_parts, total);
    assert!(
        store
            .sessions()
            .await
            .unwrap()
            .iter()
            .all(|s| s.tool != "chat")
    );
    store.close().await;
    stand.finish().await;
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn callers_that_ask_together_share_one_read_and_a_write_is_seen() {
    let stand = Stand::new("act-share").await;
    let (store, _) = stand.with_fixture().await;
    // Five callers on a cold store: one read serves them all.
    store
        .apply(&mark("w", "2026-11-11T10:00:00Z", "/w/x", "main"))
        .await
        .unwrap();
    let reads = futures_util::future::join_all((0..5).map(|_| store.session_minutes())).await;
    let reads: Vec<_> = reads.into_iter().map(Result::unwrap).collect();
    assert!(reads.iter().all(|r| std::sync::Arc::ptr_eq(r, &reads[0])));
    // The next read of the same state is the kept one.
    let again = store.session_minutes().await.unwrap();
    assert!(std::sync::Arc::ptr_eq(&again, &reads[0]));
    // A write is seen by the next read, which is a new one.
    store
        .apply(&mark("w", "2026-11-11T10:30:00Z", "/w/x", "main"))
        .await
        .unwrap();
    let after = store.session_minutes().await.unwrap();
    assert!(!std::sync::Arc::ptr_eq(&after, &reads[0]));
    let minutes = |all: &[construct_agent::activity::rules::SessionMinutes]| {
        all.iter()
            .find(|s| s.session == "w")
            .map(|s| s.minutes.len())
    };
    assert_eq!(minutes(&after), minutes(&reads[0]).map(|n| n + 1));
    store.close().await;
    stand.finish().await;
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn stored_prompt_authors_decide_whose_time_a_session_is() {
    use Author::{Person, Unattended, Unknown};
    use Role::User;
    let stand = Stand::new("act-kinds").await;
    let store = stand.store().await;
    // A script only; an unknown author only; a script beside a person. Only the first is
    // unattended: the other two count as the user's own.
    let records = [
        turn(
            "script",
            "2026-11-11T10:00:00Z",
            User,
            Unattended,
            "/work/s",
            "",
        ),
        turn(
            "unknown",
            "2026-11-11T11:00:00Z",
            User,
            Unknown,
            "/work/u",
            "",
        ),
        turn(
            "mixed",
            "2026-11-11T12:00:00Z",
            User,
            Unattended,
            "/work/m",
            "",
        ),
        turn("mixed", "2026-11-11T12:00:30Z", User, Person, "/work/m", ""),
    ];
    for record in &records {
        store.apply(record).await.unwrap();
    }
    let days = [Day {
        label: "2026-11-11".into(),
        start: minute_number("2026-11-11T00:00").unwrap(),
        end: minute_number("2026-11-12T00:00").unwrap(),
    }];
    let keys = |report: construct_agent::activity::rules::Report| -> Vec<String> {
        report.groups.into_iter().map(|g| g.key).collect()
    };
    let unattended = store
        .activity(&days, Kind::Unattended, Grouping::Project)
        .await
        .unwrap();
    assert_eq!(unattended.total_parts, PARTS_PER_MINUTE);
    assert_eq!(keys(unattended), vec!["s"]);
    let own = store
        .activity(&days, Kind::Person, Grouping::Project)
        .await
        .unwrap();
    assert_eq!(own.total_parts, 2 * PARTS_PER_MINUTE);
    assert_eq!(keys(own), vec!["m", "u"]);
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

/// Sessions the rules of the project and the ticket tell apart: a worktree folder, a branch
/// that names work, a default branch where the user's text decides, a tie, a script's prompts
/// and the assistant's text that do not count, words that are not tickets, a mark that moves a
/// session to a worktree, and folders with one name.
fn assignment_records() -> Vec<Record> {
    use Author::{Person, Unattended};
    use Role::{Assistant, User};
    let worktree = r"C:\work\app\.claude\worktrees\AB-1-first";
    let say = |session: &str, minute: u32, role, author, place: (&str, &str), text: &str| {
        said(
            session,
            &format!("2026-11-11T10:{minute:02}:00Z"),
            role,
            author,
            place,
            text,
        )
    };
    vec![
        say("w", 0, User, Person, (worktree, "master"), "do CD-2"),
        say(
            "b",
            1,
            User,
            Person,
            ("/work/app", "feature/EF-3-x"),
            "do CD-2",
        ),
        say(
            "t",
            2,
            User,
            Person,
            ("/work/app", "master"),
            "GH-4 then IJ-5",
        ),
        say(
            "t",
            3,
            Assistant,
            Person,
            ("/work/app", "master"),
            "IJ-5 IJ-5 IJ-5",
        ),
        say(
            "m",
            4,
            User,
            Person,
            ("/other/app", "main"),
            "KL-6 and MN-7",
        ),
        // The same id in a second message adds to its count.
        say(
            "m",
            4,
            User,
            Author::Unknown,
            ("/other/app", "main"),
            "KL-6 again",
        ),
        say(
            "m",
            5,
            Assistant,
            Person,
            ("/other/app", "main"),
            "MN-7 MN-7 MN-7",
        ),
        say(
            "m",
            6,
            User,
            Unattended,
            ("/other/app", "main"),
            "OP-8 OP-8 OP-8 OP-8",
        ),
        say(
            "n",
            7,
            User,
            Person,
            ("/work/tool", "HEAD"),
            "UTF-8, SHA-256, CVE-2026-1234",
        ),
        // A number too long for an id is no id and cannot break the store.
        say(
            "n",
            7,
            User,
            Person,
            ("/work/tool", "HEAD"),
            &format!("AB-{}", "7".repeat(5000)),
        ),
        say("k", 8, User, Person, ("/third/app", "master"), "QR-1"),
        // A later mark moves the session to a worktree, which then decides.
        mark(
            "k",
            "2026-11-11T10:09:00Z",
            "/third/app/.claude/worktrees/ST-2-x",
            "master",
        ),
    ]
}

fn assigned(sessions: &[Session], name: &str) -> (String, String) {
    let found = sessions.iter().find(|s| s.session == name).unwrap();
    (found.project.clone(), found.ticket.clone())
}

async fn mentions(stand: &Stand) -> Vec<(String, String, i64)> {
    sqlx::query_as("select session, ticket, count from activity.mentions order by 1, 2")
        .fetch_all(&mut stand.connect().await)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn each_session_gets_a_project_and_a_ticket() {
    let stand = Stand::new("act-assign").await;
    let store = stand.store().await;
    let records = assignment_records();
    store.rebuild(&sink_with(&stand, &records)).await.unwrap();
    let rebuilt = store.sessions().await.unwrap();

    let pair = |p: &str, t: &str| (p.to_owned(), t.to_owned());
    assert_eq!(assigned(&rebuilt, "w"), pair("C:/work/app", "AB-1"));
    assert_eq!(assigned(&rebuilt, "b"), pair("/work/app", "EF-3"));
    // The assistant's text does not count, and a tie gives none.
    assert_eq!(assigned(&rebuilt, "t"), pair("/work/app", ""));
    // Nor does a script's prompt.
    assert_eq!(assigned(&rebuilt, "m"), pair("/other/app", "KL-6"));
    assert_eq!(assigned(&rebuilt, "n"), pair("/work/tool", ""));
    // The mark of the last record moved the session, so the worktree outranks the text.
    assert_eq!(assigned(&rebuilt, "k"), pair("/third/app", "ST-2"));

    // Only ids and how often they are named are kept, never the messages.
    let counted = mentions(&stand).await;
    let expected: Vec<(String, String, i64)> = [
        ("b", "CD-2", 1),
        ("k", "QR-1", 1),
        ("m", "KL-6", 2),
        ("m", "MN-7", 1),
        ("t", "GH-4", 1),
        ("t", "IJ-5", 1),
        ("w", "CD-2", 1),
    ]
    .map(|(s, t, n)| (s.to_owned(), t.to_owned(), n))
    .to_vec();
    assert_eq!(counted, expected);

    // The same records fed one by one, backwards and twice, change nothing, counts included.
    let mut backwards = records.clone();
    backwards.reverse();
    backwards.extend(records);
    step_by_step(&stand, &store, &backwards).await;
    assert_eq!(store.sessions().await.unwrap(), rebuilt);
    assert_eq!(mentions(&stand).await, expected);
    store.close().await;
    stand.finish().await;
}

#[tokio::test]
#[ignore = "starts PostgreSQL; run with --ignored"]
async fn the_groups_by_project_and_by_ticket_use_the_stored_values() {
    let stand = Stand::new("act-assign-groups").await;
    let store = stand.store().await;
    store
        .rebuild(&sink_with(&stand, &assignment_records()))
        .await
        .unwrap();
    let day = Day {
        label: "2026-11-11".into(),
        start: minute_number("2026-11-11T00:00").unwrap(),
        end: minute_number("2026-11-12T00:00").unwrap(),
    };
    let keys = |report: construct_agent::activity::rules::Report| -> Vec<String> {
        let mut keys: Vec<String> = report.groups.into_iter().map(|g| g.key).collect();
        keys.sort();
        keys
    };
    let by_project = store
        .activity(std::slice::from_ref(&day), Kind::Both, Grouping::Project)
        .await
        .unwrap();
    // Four folders are called `app`, so each shows more of its path (`/work/app` is the end of
    // `C:/work/app`, so it shows all of it); the worktree is part of its project.
    assert_eq!(
        keys(by_project),
        ["/work/app", "C:/work/app", "other/app", "third/app", "tool"]
    );
    let by_ticket = store
        .activity(&[day], Kind::Both, Grouping::Ticket)
        .await
        .unwrap();
    assert_eq!(
        keys(by_ticket),
        ["AB-1", "EF-3", "KL-6", "No ticket", "ST-2"]
    );
    store.close().await;
    stand.finish().await;
}

/// The week of the dashboard on a store of about 140,000 prompts: the five widgets ask at once.
/// The time is of the agent's side of a click, from the five asks to the five answers. Run it
/// where the time counts, on a release build: `cargo test --release --locked --test activity --
/// --ignored --nocapture a_week_on_140000`. It prints the times and asserts none: the machine
/// decides them.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "starts PostgreSQL and builds a large store; run with --release --ignored"]
async fn a_week_on_140000_prompts_is_timed() {
    use chrono::{Duration, NaiveDate};
    use std::time::Instant;

    if cfg!(debug_assertions) {
        eprintln!("skipped: the times of a debug build say nothing; run with --release");
        return;
    }
    let stand = Stand::new("act-140k").await;
    let store = stand.store().await;
    // 1,000 sessions of 140 prompts, a minute apart, over 120 days that end on 18 November.
    let last_day = NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
    let mut records = Vec::new();
    for n in 0..1000_i64 {
        let start = (last_day - Duration::days(n % 120))
            .and_hms_opt(8 + u32::try_from(n % 8).unwrap(), 0, 10)
            .unwrap();
        for p in 0..140_i64 {
            records.push(said(
                &format!("s{n}"),
                &(start + Duration::minutes(p))
                    .format("%Y-%m-%dT%H:%M:%SZ")
                    .to_string(),
                Role::User,
                Author::Person,
                (&format!("/work/p{}/app", n % 40), "main"),
                "words",
            ));
        }
    }
    let sink = sink_with(&stand, &records);
    store.rebuild(&sink).await.unwrap();
    let days: Vec<Day> = (0..7)
        .map(|d| {
            let day = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap() + Duration::days(d);
            Day {
                label: day.to_string(),
                start: minute_number(&format!("{day}T00:00")).unwrap(),
                end: minute_number(&format!("{}T00:00", day + Duration::days(1))).unwrap(),
            }
        })
        .collect();
    let five_widgets = |store: Store| {
        let days = days.clone();
        async move {
            let started = Instant::now();
            let asks: Vec<_> = [
                Grouping::Project,
                Grouping::Ticket,
                Grouping::Tool,
                Grouping::Project,
                Grouping::Tool,
            ]
            .into_iter()
            .map(|grouping| {
                let (store, days) = (store.clone(), days.clone());
                tokio::spawn(async move {
                    store
                        .activity(&days, Kind::Both, grouping)
                        .await
                        .unwrap()
                        .total_parts
                })
            })
            .collect();
            for ask in asks {
                assert!(ask.await.unwrap() > 0);
            }
            started.elapsed()
        }
    };
    let cold = five_widgets(store.clone()).await;
    let warm = five_widgets(store.clone()).await;
    // A write at each refresh drops what was kept: the next five asks read once more.
    store
        .apply(&said(
            "s0",
            "2026-11-18T12:00:00Z",
            Role::User,
            Author::Person,
            ("/work/p0/app", "main"),
            "words",
        ))
        .await
        .unwrap();
    let after_write = five_widgets(store.clone()).await;
    println!(
        "140,000 prompts, the five widgets at once: cold {cold:?}, kept {warm:?}, after a write {after_write:?}"
    );
    store.close().await;
    stand.finish().await;
}
