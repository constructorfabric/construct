//! The activity time store: per session, the tool, folder and branch, who wrote the prompts,
//! the first and last time, a count of turns and the minutes the session was active.
//!
//! It lives in a database of its own in the agent's PostgreSQL and is only a copy: the record
//! files stay the single source, and [`Store::rebuild`] makes the whole store from them. No
//! message text is stored, only counts, times and a hash that tells a replayed turn from a new
//! one. Applying a record is idempotent and the result does not depend on the order records
//! arrive in, so a step-by-step store equals a rebuilt one.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use connectors::{Author, Mark, Record, Role, Turn, compare_times, minute_of};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{PgConnection, PgPool, Row};

use crate::database::{Error, SOCKET_PORT, SUPERUSER, err};
use crate::records::{FileSink, read_records, turn_key};

mod feed;
pub use feed::Feed;
pub mod rules;

/// The database the store lives in; the agent creates it when it is missing.
pub const DBNAME: &str = "activity";
/// The version of the rules that turn records into rows. Change it when a rule changes: the
/// next start drops the store and it is rebuilt.
pub const RULES_VERSION: i32 = 1;

/// The advisory lock `apply` and `rebuild` take for their transaction, so that no two writers
/// overlap: a record applied while the store is being cleared would be lost, and `touch` reads a
/// row and writes it back.
pub const LOCK_KEY: i64 = 0x6163_7469_7669_7479;

const SCHEMA: [&str; 4] = [
    "create table activity.rules (version int not null)",
    "create table activity.sessions (
        tool text not null,
        session text not null,
        folder text not null,
        branch text not null,
        first_time text not null,
        last_time text not null,
        turns bigint not null default 0,
        person_prompts bigint not null default 0,
        unattended_prompts bigint not null default 0,
        unknown_prompts bigint not null default 0,
        primary key (tool, session))",
    // The key of a turn, so that the same turn counts once. Never its text.
    "create table activity.turns (
        tool text not null,
        session text not null,
        key bigint not null,
        primary key (tool, session, key))",
    "create table activity.minutes (
        tool text not null,
        session text not null,
        minute text not null,
        primary key (tool, session, minute))",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub tool: String,
    pub session: String,
    /// The folder and branch of the latest record.
    pub folder: String,
    pub branch: String,
    pub first_time: String,
    pub last_time: String,
    pub turns: i64,
    /// User turns by who wrote them.
    pub person_prompts: i64,
    pub unattended_prompts: i64,
    pub unknown_prompts: i64,
    /// Distinct minutes with a turn or a mark.
    pub active_minutes: i64,
}

#[derive(Clone, Debug)]
pub struct Store {
    pool: PgPool,
}

impl Store {
    /// Connect over the PostgreSQL socket and make the schema. A store of another rules
    /// version is dropped and made again empty: call [`Store::rebuild`]
    /// after.
    ///
    /// # Errors
    /// Fails when the database cannot be reached or a statement is refused.
    pub async fn open(socket_dir: &Path) -> Result<Self, Error> {
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(Self::connect_options(socket_dir))
            .await
            .map_err(|e| err("connect to the activity database", e))?;
        let store = Self { pool };
        if let Err(e) = store.prepare().await {
            store.close().await;
            return Err(e);
        }
        Ok(store)
    }

    /// How to reach the activity database over the PostgreSQL socket.
    #[must_use]
    pub fn connect_options(socket_dir: &Path) -> PgConnectOptions {
        PgConnectOptions::new()
            .socket(socket_dir)
            .port(SOCKET_PORT)
            .username(SUPERUSER)
            .database(DBNAME)
    }

    async fn prepare(&self) -> Result<(), Error> {
        let mut tx = self.pool.begin().await.map_err(|e| err("begin", e))?;
        if stored_version(&mut tx).await? != Some(RULES_VERSION) {
            create_schema(&mut tx).await?;
        }
        tx.commit().await.map_err(|e| err("commit", e))
    }

    /// Applies one record. The same record twice changes nothing. Records that carry no
    /// activity (tool calls and results of a chat) and records without a usable time are ignored.
    ///
    /// # Errors
    /// Fails when a statement is refused.
    pub async fn apply(&self, record: &Record) -> Result<(), Error> {
        let mut tx = self.pool.begin().await.map_err(|e| err("begin", e))?;
        lock(&mut tx).await?;
        apply_in(&mut tx, record).await?;
        tx.commit().await.map_err(|e| err("commit", e))
    }

    /// Makes the whole store again from the record files, in one transaction: a reader never
    /// sees it half done. Returns how many records counted: tool records and records without a usable time do not.
    ///
    /// # Errors
    /// Fails when a file cannot be read or a statement is refused; the store is then as before.
    pub async fn rebuild(&self, sink: &FileSink) -> Result<usize, Error> {
        self.rebuild_counting(sink, &AtomicU64::new(0)).await
    }

    /// Like [`Store::rebuild`], adding every record read to `done` as it goes, so that another
    /// thread can show the progress.
    ///
    /// # Errors
    /// Fails when a file cannot be read or a statement is refused; the store is then as before.
    pub async fn rebuild_counting(
        &self,
        sink: &FileSink,
        done: &AtomicU64,
    ) -> Result<usize, Error> {
        let files = sink
            .session_files()
            .map_err(|e| err("list the record files", e))?;
        let mut tx = self.pool.begin().await.map_err(|e| err("begin", e))?;
        lock(&mut tx).await?;
        for statement in [
            "delete from activity.minutes",
            "delete from activity.turns",
            "delete from activity.sessions",
        ] {
            sqlx::query(statement)
                .execute(&mut *tx)
                .await
                .map_err(|e| err("clear the store", e))?;
        }
        let mut applied = 0;
        for file in files {
            for record in read_records(&file).map_err(|e| err("read a record file", e))? {
                applied += usize::from(apply_in(&mut tx, &record).await?);
                done.fetch_add(1, Ordering::Relaxed);
            }
        }
        tx.commit().await.map_err(|e| err("commit", e))?;
        Ok(applied)
    }

    /// Every session, ordered by tool and session.
    ///
    /// # Errors
    /// Fails when the query is refused.
    pub async fn sessions(&self) -> Result<Vec<Session>, Error> {
        let rows = sqlx::query(
            "select s.*, (select count(*) from activity.minutes m
                          where m.tool = s.tool and m.session = s.session) as active
             from activity.sessions s order by tool, session",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err("read the sessions", e))?;
        rows.iter()
            .map(|r| {
                Ok(Session {
                    tool: r.try_get("tool")?,
                    session: r.try_get("session")?,
                    folder: r.try_get("folder")?,
                    branch: r.try_get("branch")?,
                    first_time: r.try_get("first_time")?,
                    last_time: r.try_get("last_time")?,
                    turns: r.try_get("turns")?,
                    person_prompts: r.try_get("person_prompts")?,
                    unattended_prompts: r.try_get("unattended_prompts")?,
                    unknown_prompts: r.try_get("unknown_prompts")?,
                    active_minutes: r.try_get("active")?,
                })
            })
            .collect::<Result<_, sqlx::Error>>()
            .map_err(|e| err("read a session", e))
    }

    /// The active minutes, `(tool, session, minute)`, in order.
    ///
    /// # Errors
    /// Fails when the query is refused.
    pub async fn minutes(&self) -> Result<Vec<(String, String, String)>, Error> {
        sqlx::query_as("select tool, session, minute from activity.minutes order by 1, 2, 3")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| err("read the minutes", e))
    }

    /// The time of the sessions in the days the caller gives, by the rules in [`rules`]. One
    /// read, so the answer is of one state of the store.
    ///
    /// # Errors
    /// Fails when the query is refused, a stored minute cannot be read, or the days overlap.
    pub async fn activity(
        &self,
        days: &[rules::Day],
        kind: rules::Kind,
        grouping: rules::Grouping,
    ) -> Result<rules::Report, Error> {
        let rows = sqlx::query(
            "select s.tool, s.session, s.folder, s.branch, s.person_prompts,
                    s.unattended_prompts, s.unknown_prompts, m.minute
             from activity.sessions s
             join activity.minutes m on m.tool = s.tool and m.session = s.session
             order by s.tool, s.session, m.minute",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| err("read the minutes of the sessions", e))?;
        let mut sessions: Vec<rules::SessionMinutes> = Vec::new();
        for row in &rows {
            let fail = |e| err("read a minute", e);
            let tool: String = row.try_get("tool").map_err(fail)?;
            let session: String = row.try_get("session").map_err(fail)?;
            let text: String = row.try_get("minute").map_err(fail)?;
            let minute = rules::minute_number(&text)
                .ok_or_else(|| err("read a minute", format!("{text:?} is not a minute")))?;
            if let Some(last) = sessions
                .last_mut()
                .filter(|s| s.tool == tool && s.session == session)
            {
                last.minutes.push(minute);
                continue;
            }
            sessions.push(rules::SessionMinutes {
                folder: row.try_get("folder").map_err(fail)?,
                branch: row.try_get("branch").map_err(fail)?,
                unattended: rules::is_unattended(
                    row.try_get("person_prompts").map_err(fail)?,
                    row.try_get("unattended_prompts").map_err(fail)?,
                    row.try_get("unknown_prompts").map_err(fail)?,
                ),
                tool,
                session,
                minutes: vec![minute],
            });
        }
        rules::calculate(&sessions, days, kind, grouping).map_err(|e| err("calculate", e))
    }

    /// Close the connections, before PostgreSQL stops.
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

/// The stored rules version; `None` only when there is no store (no table, or no row). Any
/// other failure is an error, so a lost connection never drops a store.
async fn stored_version(conn: &mut PgConnection) -> Result<Option<i32>, Error> {
    let fail = |e| err("read the rules version", e);
    let table: Option<String> = sqlx::query_scalar("select to_regclass('activity.rules')::text")
        .fetch_one(&mut *conn)
        .await
        .map_err(fail)?;
    if table.is_none() {
        return Ok(None);
    }
    sqlx::query_scalar("select version from activity.rules")
        .fetch_optional(&mut *conn)
        .await
        .map_err(fail)
}

async fn lock(conn: &mut PgConnection) -> Result<(), Error> {
    sqlx::query("select pg_advisory_xact_lock($1)")
        .bind(LOCK_KEY)
        .execute(&mut *conn)
        .await
        .map_err(|e| err("lock the store", e))?;
    Ok(())
}

async fn create_schema(conn: &mut PgConnection) -> Result<(), Error> {
    let fail = |e| err("make the schema", e);
    sqlx::query("drop schema if exists activity cascade")
        .execute(&mut *conn)
        .await
        .map_err(fail)?;
    sqlx::query("create schema activity")
        .execute(&mut *conn)
        .await
        .map_err(fail)?;
    for statement in SCHEMA {
        sqlx::query(statement)
            .execute(&mut *conn)
            .await
            .map_err(fail)?;
    }
    sqlx::query("insert into activity.rules (version) values ($1)")
        .bind(RULES_VERSION)
        .execute(&mut *conn)
        .await
        .map_err(fail)?;
    Ok(())
}

/// Whether the record is later than the stored latest one. A total order, so the pick does not
/// depend on arrival order: equal instants (`…:05Z` and `…:05.000Z`) fall back to the text, then
/// to the folder and branch.
fn is_later(time: &str, folder: &str, branch: &str, than: (&str, &str, &str)) -> bool {
    compare_times(time, than.0)
        .then_with(|| time.cmp(than.0))
        .then_with(|| folder.cmp(than.1))
        .then_with(|| branch.cmp(than.2))
        .is_gt()
}

fn earlier(a: &str, b: &str) -> bool {
    compare_times(a, b).then_with(|| a.cmp(b)).is_lt()
}

/// The form the records promise: RFC 3339 in UTC, written with `Z` (`2026-11-11T10:15:30Z`, with
/// an optional fraction). A record whose time is not in it, or is not a real date and time, has
/// no minute to count and is left out.
fn is_utc_time(time: &str) -> bool {
    // The parser accepts any one character between the date and the time; the minute and the
    // order of times are taken from the text, so only `T` is let through.
    time.ends_with('Z')
        && time.as_bytes().get(10) == Some(&b'T')
        && time::OffsetDateTime::parse(time, &time::format_description::well_known::Rfc3339).is_ok()
}

/// Whether the store counts the record at all; tool calls and results of a chat carry no
/// activity.
fn is_activity(record: &Record) -> bool {
    match record {
        Record::Turn(_) | Record::Mark(_) => true,
        Record::ToolCall(_) | Record::ToolResult(_) => false,
    }
}

async fn apply_in(conn: &mut PgConnection, record: &Record) -> Result<bool, Error> {
    if !is_utc_time(record.time()) {
        return Ok(false);
    }
    match record {
        Record::Turn(turn) => {
            let Turn {
                tool,
                session,
                time,
                folder,
                branch,
                ..
            } = turn;
            touch(conn, tool, session, time, folder, branch).await?;
            // `bigint` is signed; the key keeps its 64 bits.
            let key = i64::from_ne_bytes(turn_key(turn).to_ne_bytes());
            let inserted = sqlx::query(
                "insert into activity.turns (tool, session, key) values ($1, $2, $3)
                 on conflict do nothing",
            )
            .bind(tool)
            .bind(session)
            .bind(key)
            .execute(&mut *conn)
            .await
            .map_err(|e| err("store a turn key", e))?
            .rows_affected();
            if inserted == 1 {
                let prompt =
                    |wanted: Author| i64::from(turn.role == Role::User && turn.author == wanted);
                sqlx::query(
                    "update activity.sessions set turns = turns + 1,
                        person_prompts = person_prompts + $3,
                        unattended_prompts = unattended_prompts + $4,
                        unknown_prompts = unknown_prompts + $5
                     where tool = $1 and session = $2",
                )
                .bind(tool)
                .bind(session)
                .bind(prompt(Author::Person))
                .bind(prompt(Author::Unattended))
                .bind(prompt(Author::Unknown))
                .execute(&mut *conn)
                .await
                .map_err(|e| err("count a turn", e))?;
            }
        }
        Record::Mark(Mark {
            tool,
            session,
            time,
            folder,
            branch,
            ..
        }) => touch(conn, tool, session, time, folder, branch).await?,
        // See `is_activity`.
        Record::ToolCall(_) | Record::ToolResult(_) => return Ok(false),
    }
    Ok(true)
}

/// The first/last time and the folder and branch are worked out in Rust from the stored row and
/// written back; the advisory lock taken by `apply` and `rebuild` keeps two writers from
/// interleaving there.
async fn touch(
    conn: &mut PgConnection,
    tool: &str,
    session: &str,
    time: &str,
    folder: &str,
    branch: &str,
) -> Result<(), Error> {
    let fail = |e| err("update a session", e);
    sqlx::query(
        "insert into activity.sessions (tool, session, folder, branch, first_time, last_time)
         values ($1, $2, $3, $4, $5, $5) on conflict do nothing",
    )
    .bind(tool)
    .bind(session)
    .bind(folder)
    .bind(branch)
    .bind(time)
    .execute(&mut *conn)
    .await
    .map_err(fail)?;
    let row = sqlx::query(
        "select folder, branch, first_time, last_time from activity.sessions
         where tool = $1 and session = $2",
    )
    .bind(tool)
    .bind(session)
    .fetch_one(&mut *conn)
    .await
    .map_err(fail)?;
    let (stored_folder, stored_branch, first, last): (String, String, String, String) = (
        row.try_get(0).map_err(fail)?,
        row.try_get(1).map_err(fail)?,
        row.try_get(2).map_err(fail)?,
        row.try_get(3).map_err(fail)?,
    );
    let (mut new_folder, mut new_branch, mut new_last) = (stored_folder, stored_branch, last);
    if is_later(time, folder, branch, (&new_last, &new_folder, &new_branch)) {
        new_folder = folder.to_owned();
        new_branch = branch.to_owned();
        time.clone_into(&mut new_last);
    }
    let new_first = if earlier(time, &first) { time } else { &first };
    sqlx::query(
        "update activity.sessions set folder = $3, branch = $4, first_time = $5, last_time = $6
         where tool = $1 and session = $2",
    )
    .bind(tool)
    .bind(session)
    .bind(new_folder)
    .bind(new_branch)
    .bind(new_first)
    .bind(new_last)
    .execute(&mut *conn)
    .await
    .map_err(fail)?;
    sqlx::query(
        "insert into activity.minutes (tool, session, minute) values ($1, $2, $3)
         on conflict do nothing",
    )
    .bind(tool)
    .bind(session)
    .bind(minute_of(time))
    .execute(&mut *conn)
    .await
    .map_err(|e| err("note a minute", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_utc_time;

    #[test]
    fn only_real_utc_times_written_with_z_count() {
        assert!(is_utc_time("2026-11-11T10:15:30Z"));
        assert!(is_utc_time("2026-11-11T10:15:30.123456Z"));
        assert!(!is_utc_time("2026-13-45T99:99:99Z"));
        assert!(!is_utc_time("2026-11-11T10:15:30+00:00"));
        assert!(!is_utc_time("2026-11-11T10:15:30+02:00"));
        assert!(!is_utc_time("2026-11-11 10:15:30Z"));
        assert!(!is_utc_time("2026-11-11t10:15:30Z"));
        assert!(!is_utc_time("2026-11-11T10:15"));
        assert!(!is_utc_time(""));
        assert!(!is_utc_time("yesterday"));
    }
}
