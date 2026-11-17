//! A scratch directory with a running PostgreSQL and the activity database, for the tests that use
//! the activity store in the test's own process. It is stopped and removed at the end.
#![allow(clippy::unwrap_used, dead_code)]

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use construct_agent::activity::{DBNAME, LOCK_KEY, Store};
use construct_agent::database::{Database, Paths, running_postmaster};
use sqlx::Connection;

pub struct PgStand {
    pub root: PathBuf,
    pub database: Database,
}

impl PgStand {
    pub async fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("cb-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths {
            data_dir: root.join("data/postgres"),
            install_dir: root.join("data/postgres-bin"),
            socket_dir: root.join("run/pg"),
        };
        let database = Database::start(paths).await.unwrap();
        database.ensure_database(DBNAME).await.unwrap();
        Self { root, database }
    }

    pub async fn store(&self) -> Store {
        Store::open(&self.database.paths().socket_dir)
            .await
            .unwrap()
    }

    /// A plain connection to the activity database, for what the store does not offer.
    pub async fn connect(&self) -> sqlx::PgConnection {
        let options = Store::connect_options(&self.database.paths().socket_dir);
        sqlx::PgConnection::connect_with(&options).await.unwrap()
    }

    /// A connection that holds the lock every writer of the store takes, until
    /// [`PgStand::release_store`]: a build or a write waits meanwhile.
    pub async fn hold_store(&self) -> sqlx::PgConnection {
        let mut holder = self.connect().await;
        sqlx::query("select pg_advisory_lock($1)")
            .bind(LOCK_KEY)
            .execute(&mut holder)
            .await
            .unwrap();
        holder
    }

    pub async fn release_store(holder: &mut sqlx::PgConnection) {
        sqlx::query("select pg_advisory_unlock($1)")
            .bind(LOCK_KEY)
            .execute(holder)
            .await
            .unwrap();
    }

    pub async fn finish(self) {
        self.database.stop().await.unwrap();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Drop for PgStand {
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

/// Appends text to a file, making it.
pub fn append(path: &Path, text: &str) {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
}
