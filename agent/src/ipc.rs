//! App <-> agent channel: one request per connection over a Unix socket in the
//! user's runtime directory. The directory and the socket are user-only (`DIR_MODE`,
//! `SOCKET_MODE`), so only the user can connect. The agent opens no TCP port.
//!
//! Protocol: the client sends one line, the agent answers one JSON line. Commands:
//! `status`, `settings-get`, and `settings-set <JSON object>` (the fields to change; see
//! `settings`). A request line is at most `MAX_REQUEST` bytes.

use std::io;
use std::path::PathBuf;

/// Socket file name inside the `construct` runtime directory.
pub const SOCKET_NAME: &str = "agent.sock";

/// The JSON line the agent answers to `status`.
#[must_use]
pub fn status_response(
    pid: u32,
    database: &str,
    construct: &str,
    connectors: &serde_json::Value,
    model: &str,
    model_name: &str,
) -> String {
    serde_json::json!({
        "status": "running",
        "version": env!("CARGO_PKG_VERSION"),
        "pid": pid,
        "database": database,
        "construct": construct,
        "connectors": connectors,
        "model": model,
        "model_name": model_name,
    })
    .to_string()
}

/// Longest request line the agent reads, in bytes. A longer one is refused.
pub const MAX_REQUEST: usize = 16 * 1024;
/// Longest command word the agent repeats in an error.
const MAX_ECHO: usize = 24;

/// The JSON line the agent answers to anything else.
///
/// The line may be a `settings-set` with a key in it, so no argument is ever repeated: the
/// command is named only when the whole line is one short word of lowercase letters, digits
/// and dashes.
#[must_use]
pub fn unknown_command_response(line: &str) -> String {
    let line = line.trim();
    let plain = !line.is_empty()
        && line.len() <= MAX_ECHO
        && line
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if plain {
        serde_json::json!({ "error": "unknown command", "command": line }).to_string()
    } else {
        serde_json::json!({ "error": "unknown command" }).to_string()
    }
}

/// The JSON line for a refused request; `message` is a fixed text.
#[must_use]
pub fn error_response(message: &str) -> String {
    serde_json::json!({ "error": message }).to_string()
}

/// The path in the environment variable `var`, when it is set to an absolute path.
#[must_use]
pub fn absolute_env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// The `construct` directory inside the runtime directory (`$XDG_RUNTIME_DIR` on
/// Linux, the per-user `$TMPDIR` on macOS).
///
/// # Errors
/// Fails when that variable is not set to an absolute path.
pub fn runtime_dir() -> io::Result<PathBuf> {
    let var = if cfg!(target_os = "macos") {
        "TMPDIR"
    } else {
        "XDG_RUNTIME_DIR"
    };
    let dir = absolute_env_path(var)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{var} is not set")))?;
    Ok(dir.join("construct"))
}

/// Socket path of the agent.
///
/// # Errors
/// Fails when the runtime directory is not set, see [`runtime_dir`].
pub fn socket_path() -> io::Result<PathBuf> {
    Ok(runtime_dir()?.join(SOCKET_NAME))
}

#[cfg(unix)]
pub use unix::{Server, create_private_dir, query_status};

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::io::{self, BufRead, BufReader, Write};
    use std::os::unix::fs::{DirBuilderExt, FileTypeExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::{MAX_REQUEST, error_response, status_response, unknown_command_response};
    use crate::database::Health;
    use crate::follow::Stats;
    use crate::model::ModelState;
    use crate::settings::{Settings, Update};

    /// Mode of the `construct` runtime directory: the user only.
    const DIR_MODE: u32 = 0o700;
    /// Mode of the socket file: the user only.
    const SOCKET_MODE: u32 = 0o600;
    /// How much of an over-long request line is read and dropped after refusing it.
    const DRAIN_LIMIT: usize = 1024 * 1024;
    /// Pause after a failed `accept`, so a persistent error (no free file
    /// descriptors) does not spin the CPU.
    const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);
    /// How long one `status` call may take in total, connect included.
    const QUERY_TIMEOUT: Duration = Duration::from_secs(5);

    /// Create `dir` (and its parents) and make it user-only.
    ///
    /// # Errors
    /// Fails when the directory cannot be created or its mode cannot be set.
    pub fn create_private_dir(dir: &Path) -> io::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(DIR_MODE)
            .create(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(DIR_MODE))
    }

    pub struct Server {
        listener: UnixListener,
        path: PathBuf,
        database: Health,
        construct: Health,
        connectors: Stats,
        model: ModelState,
        settings: Option<Settings>,
    }

    impl Server {
        /// Bind the socket, replacing a stale file left by a dead agent but
        /// refusing to replace a live one.
        ///
        /// # Errors
        /// Fails when the socket path has no parent, another agent is listening
        /// on it, or the directory or socket cannot be created.
        pub fn bind(path: &Path) -> io::Result<Self> {
            let Some(dir) = path.parent() else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "socket path has no parent directory",
                ));
            };
            create_private_dir(dir)?;
            remove_stale_socket(path)?;
            let listener = UnixListener::bind(path)?;
            if let Err(e) = fs::set_permissions(path, fs::Permissions::from_mode(SOCKET_MODE)) {
                let _ = fs::remove_file(path);
                return Err(e);
            }
            Ok(Self {
                listener,
                path: path.to_path_buf(),
                database: Health::default(),
                construct: Health::default(),
                connectors: Stats::default(),
                model: ModelState::default(),
                settings: None,
            })
        }

        /// Report this database state in `status`.
        #[must_use]
        pub fn with_database(mut self, database: Health) -> Self {
            self.database = database;
            self
        }

        /// Report this Construct host state in `status`.
        #[must_use]
        pub fn with_construct(mut self, construct: Health) -> Self {
            self.construct = construct;
            self
        }

        /// Report these log readers in `status`.
        #[must_use]
        pub fn with_connectors(mut self, connectors: Stats) -> Self {
            self.connectors = connectors;
            self
        }

        /// Report this model state in `status`: `not_configured`, `configured` or `key_rejected`.
        #[must_use]
        pub fn with_model(mut self, model: ModelState) -> Self {
            self.model = model;
            self
        }

        /// Serve `settings-get` and `settings-set` from these settings, and report their
        /// model in `status`. Without them the settings commands answer an error.
        #[must_use]
        pub fn with_settings(mut self, settings: Settings) -> Self {
            self.settings = Some(settings);
            self
        }

        #[must_use]
        pub fn path(&self) -> &Path {
            &self.path
        }

        /// Accept connections forever, one thread each.
        ///
        /// # Errors
        /// Never fails today; the signature leaves room for a fatal accept error.
        pub fn serve(&self) -> io::Result<()> {
            for conn in self.listener.incoming() {
                match conn {
                    Ok(stream) => {
                        let database = self.database.clone();
                        let construct = self.construct.clone();
                        let connectors = self.connectors.clone();
                        let model = self.model.clone();
                        let settings = self.settings.clone();
                        thread::spawn(move || {
                            let shared = Shared {
                                database: &database,
                                construct: &construct,
                                connectors: &connectors,
                                model: &model,
                                settings: settings.as_ref(),
                            };
                            if let Err(e) = handle(stream, &shared) {
                                eprintln!("connection error: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("accept error: {e}");
                        thread::sleep(ACCEPT_BACKOFF);
                    }
                }
            }
            Ok(())
        }
    }

    /// Remove a socket file left by a dead agent. A live agent is refused, and
    /// so is anything that is not a socket: the path is never unlinked blindly.
    fn remove_stale_socket(path: &Path) -> io::Result<()> {
        let meta = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        if !meta.file_type().is_socket() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} exists and is not a socket", path.display()),
            ));
        }
        match UnixStream::connect(path) {
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!("another agent is already listening on {}", path.display()),
            )),
            Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => fs::remove_file(path),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Read one request line of at most `MAX_REQUEST` bytes, without its line end.
    /// `Ok(None)` when the line is longer.
    fn read_request(stream: &UnixStream) -> io::Result<Option<Vec<u8>>> {
        let mut line = Vec::new();
        let limit = u64::try_from(MAX_REQUEST + 1).unwrap_or(u64::MAX);
        let mut reader = BufReader::new(io::Read::take(stream, limit));
        reader.read_until(b'\n', &mut line)?;
        if line.len() > MAX_REQUEST {
            discard_rest(stream);
            return Ok(None);
        }
        Ok(Some(line))
    }

    /// Read and drop the rest of an over-long line, up to `DRAIN_LIMIT` bytes, so that closing
    /// the connection does not reset it before the client has read the refusal.
    fn discard_rest(stream: &UnixStream) {
        let limit = u64::try_from(DRAIN_LIMIT).unwrap_or(u64::MAX);
        let mut reader = BufReader::new(io::Read::take(stream, limit));
        loop {
            let Ok(buf) = reader.fill_buf() else { return };
            if buf.is_empty() {
                return;
            }
            let newline = buf.contains(&b'\n');
            let n = buf.len();
            reader.consume(n);
            if newline {
                return;
            }
        }
    }

    /// What the connection threads answer from.
    struct Shared<'a> {
        database: &'a Health,
        construct: &'a Health,
        connectors: &'a Stats,
        model: &'a ModelState,
        settings: Option<&'a Settings>,
    }

    /// Never log, or return in an error, any part of `line`: it can hold a key.
    fn reply(line: &[u8], shared: &Shared) -> String {
        let settings = shared.settings;
        let Ok(line) = std::str::from_utf8(line) else {
            return error_response("invalid request");
        };
        let line = line.trim();
        let (command, argument) = match line.split_once(char::is_whitespace) {
            Some((command, rest)) => (command, rest.trim()),
            None => (line, ""),
        };
        let model_name = || {
            settings.map_or_else(
                || "not set".to_string(),
                |s| s.model().display_name().to_string(),
            )
        };
        match (command, settings) {
            ("status", _) if argument.is_empty() => status_response(
                std::process::id(),
                shared.database.get().as_str(),
                shared.construct.get().as_str(),
                &shared.connectors.to_json(),
                shared.model.get().as_str(),
                &model_name(),
            ),
            ("settings-get", Some(s)) if argument.is_empty() => s.get_response(),
            ("settings-set", Some(s)) => {
                match Update::parse(argument).and_then(|update| s.apply(update)) {
                    Ok(()) => s.get_response(),
                    Err(e) => error_response(e.as_str()),
                }
            }
            ("settings-get" | "settings-set", None) => error_response("settings are not available"),
            _ => unknown_command_response(line),
        }
    }

    fn handle(stream: UnixStream, shared: &Shared) -> io::Result<()> {
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        let reply = match read_request(&stream)? {
            Some(line) => reply(&line, shared),
            None => error_response("request too long"),
        };
        let mut stream = stream;
        writeln!(stream, "{reply}")
    }

    /// Ask the agent on `path` for its status; returns the JSON line. The whole
    /// call, connect included, gives up after `QUERY_TIMEOUT`.
    ///
    /// # Errors
    /// Fails when nothing listens on `path`, the agent does not answer in time, or it
    /// sends an empty reply.
    pub fn query_status(path: &Path) -> io::Result<String> {
        let (tx, rx) = mpsc::channel();
        let path = path.to_path_buf();
        thread::spawn(move || {
            let _ = tx.send(ask_status(&path));
        });
        rx.recv_timeout(QUERY_TIMEOUT).unwrap_or_else(|_| {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the agent did not answer in time",
            ))
        })
    }

    fn ask_status(path: &Path) -> io::Result<String> {
        let mut stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(QUERY_TIMEOUT))?;
        stream.set_write_timeout(Some(QUERY_TIMEOUT))?;
        stream.write_all(b"status\n")?;
        let mut reply = String::new();
        if BufReader::new(stream).read_line(&mut reply)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the agent closed the connection without a reply",
            ));
        }
        Ok(reply.trim().to_string())
    }
}
