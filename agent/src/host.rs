//! The Construct host: the toolkit runtime with Construct, graph storage, the types registry,
//! the authz and authn resolvers (single-user static plugins) and the API gateway, started
//! inside the agent the way the cloud host starts them.
//!
//! The REST interface listens on `127.0.0.1` on a port the agent picks, and every call needs
//! the token the agent keeps in a user-only file. The app learns the port and the token from
//! `connection.json` in the runtime directory. Graph storage uses the agent's PostgreSQL on
//! its Unix socket; Construct's own tables are a `SQLite` file in the data directory.

// The gears are found through `inventory`: linking their crates is what registers them.
use api_gateway as _;
use authn_resolver as _;
use authz_resolver as _;
use construct as _;
use graph_storage as _;
use static_authn_plugin as _;
use static_authz_plugin as _;
use types_registry as _;

use std::fs;
use std::io::{self, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use figment::Figment;
use figment::providers::Serialized;
use oagw_sdk::ServiceGatewayClientV1;
use serde_json::json;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use toolkit::bootstrap::AppConfig;
use toolkit::runtime::{ClientRegistration, DbOptions, RunOptions, ShutdownOptions};
use toolkit_security::constants::DEFAULT_SUBJECT_ID;

use crate::database::{Paths as DatabasePaths, SOCKET_PORT};
use crate::ipc::{absolute_env_path, create_private_dir, runtime_dir};
use crate::model::{ModelGateway, ModelState, construct_section};
use crate::settings::Settings;

/// Name of the file with the token, in the host's data directory.
const TOKEN_FILE: &str = "token";
/// Name of the file the app reads: the port and the token, in the runtime directory.
pub const CONNECTION_FILE: &str = "connection.json";
/// Mode of the files that hold the token: the user only.
const FILE_MODE: u32 = 0o600;
/// Token size in bytes; the token is the same bytes as hex.
const TOKEN_BYTES: usize = 32;
/// The service account Construct runs its profile reads and writes as. It is not a person.
const PROCESSING_IDENTITY: &str = "22222222-0000-4000-8000-000000000001";
/// The database graph storage keeps its tables in; the agent creates it when it is missing.
pub const GRAPH_DBNAME: &str = "graph_storage";
/// How long the host may take to answer its first call.
const READY_TIMEOUT: Duration = Duration::from_secs(120);
/// How long the host may take to stop.
const STOP_TIMEOUT: Duration = Duration::from_secs(40);

/// Where the host keeps its files.
#[derive(Clone, Debug)]
pub struct Paths {
    /// The toolkit's home directory: Construct's `SQLite` file and the token.
    pub home_dir: PathBuf,
    /// Directory of the connection file; user-only.
    pub runtime_dir: PathBuf,
}

impl Paths {
    /// `$XDG_DATA_HOME/construct/host` (default `~/.local/share/construct/host`) and the
    /// agent's runtime directory.
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
        Ok(Self {
            home_dir: data_home.join("construct").join("host"),
            runtime_dir: runtime_dir()?,
        })
    }

    #[must_use]
    pub fn token_file(&self) -> PathBuf {
        self.home_dir.join(TOKEN_FILE)
    }

    #[must_use]
    pub fn connection_file(&self) -> PathBuf {
        self.runtime_dir.join(CONNECTION_FILE)
    }
}

/// What the app needs to call the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    pub port: u16,
    pub token: String,
}

impl Connection {
    /// The URL of the profile read of the static identity.
    #[must_use]
    pub fn profile_url(&self) -> String {
        format!(
            "http://127.0.0.1:{}/construct/v1/subjects/{DEFAULT_SUBJECT_ID}/profile",
            self.port
        )
    }

    /// The URL of the manage-facts call of the static identity.
    #[must_use]
    pub fn manage_url(&self) -> String {
        format!(
            "http://127.0.0.1:{}/construct/v1/subjects/{DEFAULT_SUBJECT_ID}/facts/manage",
            self.port
        )
    }

    /// The JSON the app reads.
    #[must_use]
    pub fn to_json(&self) -> String {
        json!({
            "url": format!("http://127.0.0.1:{}", self.port),
            "port": self.port,
            "token": self.token,
        })
        .to_string()
    }
}

/// A running host.
pub struct Host {
    cancel: CancellationToken,
    task: JoinHandle<anyhow::Result<()>>,
    connection: Connection,
    connection_file: PathBuf,
    /// The task ended and was read; it cannot be awaited again.
    finished: bool,
}

impl Host {
    /// Start the host and wait until it answers an authenticated profile read. The host calls
    /// the model of `settings`, read on every call, for the planner. Without settings (the file
    /// could not be opened) the host has no model and takes records and drops them.
    /// `abort` cancels the start: the host that was begun is stopped (and awaited) before this
    /// returns, so the caller may stop the database right after.
    ///
    /// # Errors
    /// Fails when the files cannot be written, the configuration is refused, the host stops
    /// while starting, it does not answer in time, or the start is aborted.
    pub async fn start(
        paths: &Paths,
        database: &DatabasePaths,
        model: Option<(&Settings, &ModelState)>,
        abort: &CancellationToken,
    ) -> anyhow::Result<Self> {
        let connection_file = paths.connection_file();
        create_private_dir(&paths.runtime_dir)?;
        // A file left by a host that is gone names a dead port next to a token nobody serves.
        remove_if_present(&connection_file)?;
        create_private_dir(&paths.home_dir)?;
        let token = read_or_create_token(&paths.token_file())?;
        let port = free_loopback_port()?;
        let config = build_config(
            &paths.home_dir,
            &database.socket_dir,
            port,
            &token,
            model.is_some(),
        )?;
        let figment = Figment::new().merge(Serialized::defaults(&config));
        let db = toolkit_db::DbManager::from_figment(figment, config.server.home_dir.clone())?;

        let cancel = CancellationToken::new();
        let mut options = RunOptions::new(
            Arc::new(config),
            DbOptions::Manager(Arc::new(db)),
            ShutdownOptions::Token(cancel.clone()),
            uuid::Uuid::new_v4(),
        );
        // The planner finds its model client in the client hub on every call.
        if let Some((settings, state)) = model {
            let gateway: Arc<dyn ServiceGatewayClientV1> =
                Arc::new(ModelGateway::new(settings.clone(), state.clone())?);
            options = options.with_clients(vec![ClientRegistration::new(gateway)]);
        }
        let mut task = tokio::spawn(toolkit::runtime::run(options));
        // Any other way out of this function (a panic, an early return) still stops the host.
        let guard = cancel.clone().drop_guard();
        let connection = Connection { port, token };

        let ready = tokio::select! {
            res = &mut task => {
                guard.disarm();
                return Err(match res {
                    Ok(Ok(())) => anyhow::anyhow!("the host stopped while starting"),
                    Ok(Err(e)) => e.context("the host failed while starting"),
                    Err(e) => anyhow::anyhow!("the host task failed: {e}"),
                });
            }
            () = abort.cancelled() => Err(anyhow::anyhow!("the start was aborted")),
            ready = wait_until_ready(&connection) => ready,
        };
        // Only a host that answers is announced to the app, and the file is written while
        // the guard still stops the host if that fails.
        let announced = ready.and_then(|()| {
            write_private(&connection_file, connection.to_json().as_bytes()).map_err(Into::into)
        });
        if let Err(e) = announced {
            cancel.cancel();
            let _ = tokio::time::timeout(STOP_TIMEOUT, &mut task).await;
            let _ = fs::remove_file(&connection_file);
            guard.disarm();
            return Err(e);
        }
        guard.disarm();
        Ok(Self {
            cancel,
            task,
            connection,
            connection_file,
            finished: false,
        })
    }

    #[must_use]
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Resolves when the host stops on its own.
    ///
    /// # Errors
    /// Fails when the host stopped with an error or its task failed.
    pub async fn stopped(&mut self) -> anyhow::Result<()> {
        let res = self.join().await;
        self.finished = true;
        // The host is gone: its port and token must not stay readable.
        let _ = fs::remove_file(&self.connection_file);
        res
    }

    async fn join(&mut self) -> anyhow::Result<()> {
        match (&mut self.task).await {
            Ok(res) => res,
            Err(e) => Err(anyhow::anyhow!("the host task failed: {e}")),
        }
    }

    /// Stop the host and remove the connection file.
    ///
    /// # Errors
    /// Fails when the host does not stop in time or stops with an error.
    pub async fn stop(mut self) -> anyhow::Result<()> {
        let _ = fs::remove_file(&self.connection_file);
        if self.finished {
            return Ok(());
        }
        self.cancel.cancel();
        match tokio::time::timeout(STOP_TIMEOUT, &mut self.task).await {
            Ok(Ok(res)) => res,
            Ok(Err(e)) => Err(anyhow::anyhow!("the host task failed: {e}")),
            Err(_) => {
                self.task.abort();
                Err(anyhow::anyhow!("the host did not stop in time"))
            }
        }
    }
}

/// The toolkit configuration of the host, built in memory.
///
/// The token goes into the static authn plugin as a literal: the plugin expands `${NAME}`
/// from the environment only for values that contain it, and the agent does not change
/// its own environment.
///
/// # Errors
/// Fails when the toolkit refuses the configuration.
pub fn build_config(
    home_dir: &Path,
    socket_dir: &Path,
    port: u16,
    token: &str,
    with_model: bool,
) -> anyhow::Result<AppConfig> {
    let mut value = json!({
        "server": { "name": "construct-agent", "home_dir": home_dir },
        "database": {
            "auto_provision": true,
            "servers": {
                "sqlite_construct": {
                    "engine": "sqlite",
                    "params": { "WAL": "true", "synchronous": "NORMAL", "busy_timeout": "5000" },
                    "pool": { "max_conns": 5, "acquire_timeout": "30s" },
                },
                // The socket directory as host: the sqlx convention for a Unix socket.
                "pg_graph": {
                    "engine": "postgres",
                    "host": socket_dir,
                    "port": SOCKET_PORT,
                    "user": "postgres",
                },
            },
        },
        "gears": {
            "api-gateway": { "config": {
                "bind_addr": format!("127.0.0.1:{port}"),
                "enable_docs": false,
                "cors_enabled": false,
                "auth_disabled": false,
            } },
            // The embedded types registry keeps its entities in its own SQLite file.
            "types-registry": {
                "database": { "server": "sqlite_construct", "file": "types_registry.db" },
            },
            "authn-resolver": { "config": { "vendor": "constructorfabric" } },
            "authz-resolver": { "config": { "vendor": "constructorfabric" } },
            "static-authn-plugin": { "config": {
                "vendor": "constructorfabric",
                "priority": 100,
                "mode": "static_tokens",
                "tokens": [ { "token": token, "identity": {} } ],
            } },
            "static-authz-plugin": { "config": { "vendor": "constructorfabric", "priority": 100 } },
            "graph-storage": {
                "database": { "server": "pg_graph", "dbname": GRAPH_DBNAME },
                // The fake provider ranks nothing; Construct reads and writes by key.
                "config": { "embedding_dimension": 384, "embedding_provider": "fake" },
            },
            "construct": {
                "database": { "server": "sqlite_construct", "file": "construct.db" },
                "config": {
                    "processing_identity": PROCESSING_IDENTITY,
                    "profile_category_check": "per_category",
                },
            },
        },
    });
    if with_model {
        // The model is read from the settings on every call, so the host is set up for one
        // even before the user has saved it. Records are processed with this switch: no
        // sensitive-data check exists yet, and the user's own device holds the user's own data.
        let config = &mut value["gears"]["construct"]["config"];
        config["model"] = construct_section();
        config["processing"] = json!({ "run_without_sensitive_data_checks": true });
    }
    Ok(serde_json::from_value(value)?)
}

/// A random token: 32 bytes as 64 hex characters.
///
/// # Errors
/// Fails when the system's random source cannot be read.
pub fn generate_token() -> io::Result<String> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    Ok(hex::encode(bytes))
}

fn is_token(s: &str) -> bool {
    s.len() == TOKEN_BYTES * 2 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The token in `file`, or a new one written there when the file is missing or damaged.
///
/// # Errors
/// Fails when the file cannot be read or written.
pub fn read_or_create_token(file: &Path) -> io::Result<String> {
    match fs::read_to_string(file) {
        Ok(s) if is_token(s.trim()) => return Ok(s.trim().to_string()),
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let token = generate_token()?;
    write_private(file, token.as_bytes())?;
    Ok(token)
}

/// Write `data` to `file` so that it is never readable by anyone else: a user-only
/// temporary file, renamed into place.
///
/// # Errors
/// Fails when the file cannot be written.
pub fn write_private(file: &Path, data: &[u8]) -> io::Result<()> {
    let tmp = file.with_extension("tmp");
    let _ = fs::remove_file(&tmp);
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(FILE_MODE)
        .open(&tmp)?;
    f.write_all(data)?;
    f.sync_all()?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(FILE_MODE))?;
    fs::rename(&tmp, file)
}

/// Remove `file`; a missing file is fine.
fn remove_if_present(file: &Path) -> io::Result<()> {
    match fs::remove_file(file) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// A port nothing listens on at the moment, on the loopback address.
fn free_loopback_port() -> io::Result<u16> {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
    Ok(listener.local_addr()?.port())
}

/// An HTTP client for the host's loopback port. It takes no proxy from the environment, because
/// its calls carry the bearer token, and it hands back a 4xx or 5xx as an answer, not an error.
pub(crate) fn loopback_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .proxy(None)
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into()
}

/// The HTTP status of the host's answer to an authenticated profile read of the static
/// identity, or `None` while the host does not answer.
async fn profile_status(connection: &Connection) -> Option<u16> {
    let url = connection.profile_url();
    let token = connection.token.clone();
    tokio::task::spawn_blocking(move || {
        loopback_agent(Duration::from_secs(5))
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .call()
            .ok()
            .map(|response| response.status().as_u16())
    })
    .await
    .ok()
    .flatten()
}

/// Poll until the host answers its first authenticated call with `200`.
async fn wait_until_ready(connection: &Connection) -> anyhow::Result<()> {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if profile_status(connection).await == Some(200) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            anyhow::bail!(
                "the host did not answer a profile read within {} seconds",
                READY_TIMEOUT.as_secs()
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[cfg(test)]
// A failed `unwrap` in a test is a failed test.
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cb-host-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_token_is_64_hex_characters_and_never_repeats() {
        let a = generate_token().unwrap();
        assert!(is_token(&a), "{a}");
        assert_ne!(a, generate_token().unwrap());
    }

    #[test]
    fn the_token_file_is_user_only_and_the_token_is_reused() {
        let dir = scratch("token");
        let file = dir.join("token");
        let first = read_or_create_token(&file).unwrap();
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(read_or_create_token(&file).unwrap(), first);
        // A damaged file is replaced.
        fs::write(&file, "not a token").unwrap();
        assert!(is_token(&read_or_create_token(&file).unwrap()));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_config_binds_loopback_and_uses_the_socket_directory() {
        let config = build_config(
            Path::new("/data/host"),
            Path::new("/run/construct/pg"),
            4321,
            &"ab".repeat(32),
            false,
        )
        .unwrap();
        let gateway = &config.gears["api-gateway"]["config"];
        assert_eq!(gateway["bind_addr"], "127.0.0.1:4321");
        assert_eq!(gateway["auth_disabled"], false);
        let servers = &config.database.as_ref().unwrap().servers;
        assert_eq!(
            servers["pg_graph"].host.as_deref(),
            Some("/run/construct/pg")
        );
        assert_eq!(
            config.gears["static-authn-plugin"]["config"]["mode"],
            "static_tokens"
        );
    }

    #[tokio::test]
    async fn a_host_that_ends_by_itself_takes_its_connection_file_with_it() {
        let dir = scratch("stopped");
        let file = dir.join(CONNECTION_FILE);
        let connection = Connection {
            port: 1,
            token: "t".into(),
        };
        write_private(&file, connection.to_json().as_bytes()).unwrap();
        let mut host = Host {
            cancel: CancellationToken::new(),
            task: tokio::spawn(async { Ok(()) }),
            connection,
            connection_file: file.clone(),
            finished: false,
        };
        host.stopped().await.unwrap();
        assert!(!file.exists(), "the file of a dead host is left behind");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_config_turns_processing_on_and_names_the_route_but_no_key() {
        let config = build_config(
            Path::new("/data/host"),
            Path::new("/run/construct/pg"),
            4321,
            &"ab".repeat(32),
            true,
        )
        .unwrap();
        let construct = &config.gears["construct"]["config"];
        assert_eq!(construct["model"]["upstream_alias"], "model");
        // Construct limits the whole call, which makes three attempts of 60 s.
        assert_eq!(
            construct["model"]["timeout"],
            format!("{}s", crate::model::call_budget_secs())
        );
        assert!(crate::model::call_budget_secs() > 3 * 60);
        assert_eq!(
            construct["processing"]["run_without_sensitive_data_checks"],
            true
        );
        assert_eq!(construct["processing_identity"], PROCESSING_IDENTITY);
    }

    #[test]
    fn without_a_model_the_config_has_no_model_and_no_switch() {
        let config = build_config(Path::new("/d"), Path::new("/s"), 1, "t", false).unwrap();
        let construct = &config.gears["construct"]["config"];
        assert!(construct.get("model").is_none());
        assert!(construct.get("processing").is_none());
    }

    #[test]
    fn the_connection_file_names_the_port_and_the_token() {
        let connection = Connection {
            port: 4321,
            token: "t".into(),
        };
        let v: serde_json::Value = serde_json::from_str(&connection.to_json()).unwrap();
        assert_eq!(v["port"], 4321);
        assert_eq!(v["token"], "t");
        assert_eq!(v["url"], "http://127.0.0.1:4321");
    }
}
