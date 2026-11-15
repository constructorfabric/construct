//! The agent's own PostgreSQL: bundled binaries, started as a child of the agent
//! on a Unix socket only (no TCP), with its data in the user's data directory.
//!
//! The data directory outlives the agent and the service: stopping the agent or
//! removing the service keeps it. pgvector is not part of the PostgreSQL
//! binaries; the agent carries its own build of it (see `pgvector`) and copies it in at start.

use std::fmt;
use std::fs;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use postgresql_embedded::{PostgreSQL, Settings, VersionReq};

use crate::ipc::{absolute_env_path, create_private_dir};

/// The PostgreSQL release bundled into the binary (set in `.cargo/config.toml`).
const POSTGRESQL_VERSION: &str = env!("POSTGRESQL_VERSION");
/// Port number used only to name the socket file (`.s.PGSQL.5432`); nothing listens on TCP.
pub const SOCKET_PORT: u16 = 5432;
/// Only the local socket is reachable, and only by the user (the socket directory is `0700`):
/// the user connects as the superuser `postgres` without a password.
const PG_HBA: &str = "local all all trust\n";
/// Written into the installation directory once the unpack is complete. Without it an
/// unpack that was killed half way would be trusted as done.
const INSTALL_DONE: &str = ".complete";
/// Written into the data directory once PostgreSQL has started on it for the first time.
/// A data directory without it comes from a first run that never finished and is rebuilt.
const DATA_STARTED: &str = "construct-started";
/// How often the agent checks that PostgreSQL still accepts connections.
const WATCH_INTERVAL: Duration = Duration::from_secs(2);
/// Failed checks in a row before the database counts as gone.
const WATCH_FAILURES: u32 = 3;

/// What the agent tells in `status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Unpacking, initializing or starting; `status` does not wait for it.
    Starting,
    Ready,
    Failed,
}

impl State {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            State::Starting => "starting",
            State::Ready => "ready",
            State::Failed => "failed",
        }
    }
}

/// The database state, shared between the starting task and the socket threads.
#[derive(Clone, Debug)]
pub struct Health(Arc<Mutex<State>>);

impl Default for Health {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(State::Starting)))
    }
}

impl Health {
    #[must_use]
    pub fn get(&self) -> State {
        *crate::sync::lock(&self.0)
    }

    pub fn set(&self, state: State) {
        *crate::sync::lock(&self.0) = state;
    }
}

/// Where the database lives.
#[derive(Clone, Debug)]
pub struct Paths {
    /// PostgreSQL data directory; kept across restarts and `uninstall-service`.
    pub data_dir: PathBuf,
    /// Unpacked PostgreSQL binaries (and pgvector); can be rebuilt.
    pub install_dir: PathBuf,
    /// Directory of the Unix socket; user-only.
    pub socket_dir: PathBuf,
}

impl Paths {
    /// `$XDG_DATA_HOME/construct/postgres` (default `~/.local/share/construct/postgres`)
    /// and a `pg` directory next to the agent socket.
    ///
    /// # Errors
    /// Fails when `HOME` or the runtime directory is not set.
    pub fn from_env() -> io::Result<Self> {
        let data_home = match absolute_env_path("XDG_DATA_HOME") {
            Some(dir) => dir,
            None => absolute_env_path("HOME")
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?
                .join(".local")
                .join("share"),
        };
        let root = data_home.join("construct");
        let runtime = crate::ipc::runtime_dir()?;
        Ok(Self {
            data_dir: root.join("postgres"),
            install_dir: root.join("postgres-bin"),
            socket_dir: runtime.join("pg"),
        })
    }

    /// The socket file PostgreSQL listens on.
    #[must_use]
    pub fn socket_file(&self) -> PathBuf {
        self.socket_dir.join(format!(".s.PGSQL.{SOCKET_PORT}"))
    }
}

#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn err(context: &str, e: impl fmt::Display) -> Error {
    Error(format!("{context}: {e}"))
}

/// A running PostgreSQL. Call [`Database::stop`] before the agent exits.
pub struct Database {
    postgres: PostgreSQL,
    paths: Paths,
}

fn settings(paths: &Paths) -> Result<Settings, Error> {
    let version = VersionReq::parse(POSTGRESQL_VERSION)
        .map_err(|e| err("bad bundled PostgreSQL version", e))?;
    let mut s = Settings::new();
    s.version = version;
    s.installation_dir.clone_from(&paths.install_dir);
    s.data_dir.clone_from(&paths.data_dir);
    // The superuser password is only used by `initdb`; the socket needs none.
    s.password_file = paths.socket_dir.join("initdb.pw");
    s.host = "localhost".into();
    s.port = SOCKET_PORT;
    s.temporary = false;
    // The crate starts PostgreSQL with `-F` (fsync off); the user's data is kept, so turn it on.
    s.configuration.insert("fsync".into(), "on".into());
    // No TCP, and a user-only socket file.
    s.configuration
        .insert("listen_addresses".into(), "''".into());
    s.configuration
        .insert("unix_socket_permissions".into(), "0700".into());
    s.socket_dir = Some(paths.socket_dir.clone());
    // `initdb` and a first start unpack and create files; give them time.
    s.timeout = Some(std::time::Duration::from_secs(120));
    Ok(s)
}

fn write_hba(data_dir: &Path) -> io::Result<()> {
    fs::write(data_dir.join("pg_hba.conf"), PG_HBA)
}

/// Remove what a first run that was killed half way left behind: an installation without
/// its completion mark, and a data directory PostgreSQL never started on.
fn discard_partial_setup(paths: &Paths) -> io::Result<()> {
    if paths.install_dir.exists() && !paths.install_dir.join(INSTALL_DONE).exists() {
        fs::remove_dir_all(&paths.install_dir)?;
    }
    if paths.data_dir.exists() && !paths.data_dir.join(DATA_STARTED).exists() {
        fs::remove_dir_all(&paths.data_dir)?;
    }
    Ok(())
}

/// After a failed `setup()`: remove the installation, which is rebuilt from the binary at the
/// next start. The data directory is left alone, it may hold the user's database; one that
/// never started is removed by `discard_partial_setup`.
fn discard_failed_unpack(paths: &Paths) {
    let _ = fs::remove_dir_all(&paths.install_dir);
}

/// The PID in `postmaster.pid`, when it belongs to a running `postgres` that was started on
/// this data directory. A stale file whose PID the system has given to another program
/// does not match, so that program is never stopped.
#[must_use]
pub fn running_postmaster(data_dir: &Path) -> Option<u32> {
    let text = fs::read_to_string(data_dir.join("postmaster.pid")).ok()?;
    let pid: u32 = text.lines().next()?.trim().parse().ok()?;
    let cmdline = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let args: Vec<String> = cmdline
        .split(|b| *b == 0)
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    let program = args.first()?;
    if Path::new(program).file_name()? != "postgres" {
        return None;
    }
    let dir = args
        .iter()
        .position(|a| a == "-D")
        .and_then(|i| args.get(i + 1))?;
    (Path::new(dir) == data_dir).then_some(pid)
}

impl Database {
    /// Unpack and initialize (first run), copy pgvector in, stop a leftover PostgreSQL of a
    /// dead agent, start, and check that pgvector is available.
    ///
    /// # Errors
    /// Fails when a step cannot be done; the message names the step.
    pub async fn start(paths: Paths) -> Result<Self, Error> {
        create_private_dir(&paths.socket_dir).map_err(|e| err("socket directory", e))?;
        if let Some(parent) = paths.data_dir.parent() {
            create_private_dir(parent).map_err(|e| err("data directory", e))?;
        }
        discard_partial_setup(&paths).map_err(|e| err("discard a partial first run", e))?;
        let mut postgres = PostgreSQL::new(settings(&paths)?);
        if let Err(e) = postgres.setup().await {
            discard_failed_unpack(&paths);
            return Err(err("set up PostgreSQL", e));
        }
        fs::write(paths.install_dir.join(INSTALL_DONE), "").map_err(|e| err("mark install", e))?;
        write_hba(&paths.data_dir).map_err(|e| err("write pg_hba.conf", e))?;
        let _ = fs::remove_file(paths.socket_dir.join("initdb.pw"));

        let installation = postgres.settings().installation_dir.clone();
        let release = installation
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        crate::pgvector::install(&installation, &release)
            .map_err(|e| err("install pgvector", e))?;

        // A `postmaster.pid` of a running PostgreSQL means its agent was killed outside
        // systemd. It is stopped first, so this agent owns the process. A stale file is
        // cleaned up by PostgreSQL itself.
        if let Some(pid) = running_postmaster(&paths.data_dir) {
            postgres
                .stop()
                .await
                .map_err(|e| err(&format!("stop the old PostgreSQL (pid {pid})"), e))?;
        }
        postgres
            .start()
            .await
            .map_err(|e| err("start PostgreSQL", e))?;
        fs::write(paths.data_dir.join(DATA_STARTED), "").map_err(|e| err("mark data", e))?;
        let database = Self { postgres, paths };
        database.check_pgvector().await?;
        Ok(database)
    }

    #[must_use]
    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// Ask PostgreSQL, over the socket, for the pgvector it offers; it must be the
    /// version the agent carries.
    async fn check_pgvector(&self) -> Result<(), Error> {
        let want = crate::pgvector::version().map_err(|e| err("pgvector", e))?;
        let out = tokio::process::Command::new(self.postgres.settings().binary_dir().join("psql"))
            .arg("-h")
            .arg(&self.paths.socket_dir)
            .args(["-U", "postgres", "-d", "postgres", "-Atc"])
            .arg("select default_version from pg_available_extensions where name = 'vector'")
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|e| err("run psql", e))?;
        let got = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && got == want {
            Ok(())
        } else {
            Err(Error(format!(
                "pgvector check: wanted {want}, got '{got}' {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }

    /// Create the database `name` unless it exists; the Construct host's graph storage
    /// does not create its own.
    ///
    /// # Errors
    /// Fails when PostgreSQL refuses the check or the creation.
    pub async fn ensure_database(&self, name: &str) -> Result<(), Error> {
        let exists = self
            .postgres
            .database_exists(name)
            .await
            .map_err(|e| err(&format!("check database {name}"), e))?;
        if exists {
            return Ok(());
        }
        self.postgres
            .create_database(name)
            .await
            .map_err(|e| err(&format!("create database {name}"), e))
    }

    /// Whether PostgreSQL accepts connections on its socket.
    #[must_use]
    pub fn is_up(&self) -> bool {
        UnixStream::connect(self.paths.socket_file()).is_ok()
    }

    /// Returns once PostgreSQL has stopped accepting connections for several checks in a row.
    pub async fn watch(&self) {
        let mut failures = 0;
        while failures < WATCH_FAILURES {
            tokio::time::sleep(WATCH_INTERVAL).await;
            failures = if self.is_up() { 0 } else { failures + 1 };
        }
    }

    /// Stop PostgreSQL (fast shutdown) and wait for it.
    ///
    /// # Errors
    /// Fails when `pg_ctl stop` fails.
    pub async fn stop(&self) -> Result<(), Error> {
        self.postgres
            .stop()
            .await
            .map_err(|e| err("stop PostgreSQL", e))
    }
}

/// Stop the PostgreSQL that runs on `paths.data_dir`, if there is one; used when the agent
/// is told to stop while it is still starting up.
pub async fn stop_leftover(paths: &Paths) {
    let Some(pid) = running_postmaster(&paths.data_dir) else {
        return;
    };
    let result = match settings(paths) {
        Ok(s) => PostgreSQL::new(s).stop().await.map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    if let Err(e) = result {
        eprintln!("could not stop PostgreSQL (pid {pid}): {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cb-db-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn settings_keep_postgresql_off_the_network() {
        let paths = Paths {
            data_dir: "/d".into(),
            install_dir: "/i".into(),
            socket_dir: "/s".into(),
        };
        let s = settings(&paths).unwrap();
        assert_eq!(s.configuration["listen_addresses"], "''");
        assert_eq!(s.configuration["unix_socket_permissions"], "0700");
        assert_eq!(s.configuration["fsync"], "on");
        assert_eq!(s.socket_dir, Some(PathBuf::from("/s")));
    }

    #[test]
    fn hba_allows_only_the_local_socket() {
        let dir = scratch("hba");
        write_hba(&dir).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("pg_hba.conf")).unwrap(),
            "local all all trust\n"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_pid_file_of_another_program_is_not_a_running_postmaster() {
        let dir = scratch("pid");
        // This very test process is alive, but it is not a postgres.
        fs::write(
            dir.join("postmaster.pid"),
            format!("{}\n{}\n", std::process::id(), dir.display()),
        )
        .unwrap();
        assert_eq!(running_postmaster(&dir), None);
        // No such process, no such file.
        fs::write(dir.join("postmaster.pid"), "4194300\n").unwrap();
        assert_eq!(running_postmaster(&dir), None);
        fs::remove_file(dir.join("postmaster.pid")).unwrap();
        assert_eq!(running_postmaster(&dir), None);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_first_run_that_never_finished_is_discarded() {
        let root = scratch("partial");
        let paths = Paths {
            data_dir: root.join("postgres"),
            install_dir: root.join("postgres-bin"),
            socket_dir: root.join("pg"),
        };
        for dir in [&paths.data_dir, &paths.install_dir] {
            fs::create_dir_all(dir).unwrap();
        }
        discard_partial_setup(&paths).unwrap();
        assert!(!paths.data_dir.exists());
        assert!(!paths.install_dir.exists());

        // With the completion marks the directories stay.
        for (dir, mark) in [
            (&paths.data_dir, DATA_STARTED),
            (&paths.install_dir, INSTALL_DONE),
        ] {
            fs::create_dir_all(dir).unwrap();
            fs::write(dir.join(mark), "").unwrap();
        }
        discard_partial_setup(&paths).unwrap();
        assert!(paths.data_dir.exists() && paths.install_dir.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_failed_unpack_keeps_a_database_that_has_started() {
        let root = scratch("unpack");
        let paths = Paths {
            data_dir: root.join("postgres"),
            install_dir: root.join("postgres-bin"),
            socket_dir: root.join("pg"),
        };
        fs::create_dir_all(&paths.install_dir).unwrap();
        fs::create_dir_all(paths.data_dir.join("base")).unwrap();
        fs::write(paths.data_dir.join(DATA_STARTED), "").unwrap();

        discard_failed_unpack(&paths);
        assert!(!paths.install_dir.exists());
        assert!(paths.data_dir.join("base").exists());
        // The next start keeps it too.
        discard_partial_setup(&paths).unwrap();
        assert!(paths.data_dir.join("base").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn health_starts_as_starting_and_is_shared() {
        let health = Health::default();
        let other = health.clone();
        assert_eq!(health.get(), State::Starting);
        other.set(State::Ready);
        assert_eq!(health.get().as_str(), "ready");
    }
}
