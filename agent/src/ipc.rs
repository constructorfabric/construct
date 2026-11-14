//! App <-> agent channel: one request per connection over a Unix socket in the
//! user's runtime directory. The directory and the socket are user-only (`DIR_MODE`,
//! `SOCKET_MODE`), so only the user can connect. The agent opens no TCP port.
//!
//! Protocol: the client sends a line (`status`), the agent answers one JSON line.

use std::io;
use std::path::PathBuf;

/// Socket file name inside the `construct` runtime directory.
pub const SOCKET_NAME: &str = "agent.sock";

/// The JSON line the agent answers to `status`.
#[must_use]
pub fn status_response(pid: u32) -> String {
    serde_json::json!({
        "status": "running",
        "version": env!("CARGO_PKG_VERSION"),
        "pid": pid,
    })
    .to_string()
}

/// The JSON line the agent answers to anything else.
#[must_use]
pub fn unknown_command_response(command: &str) -> String {
    serde_json::json!({ "error": "unknown command", "command": command }).to_string()
}

/// Socket path from the runtime directory (`$XDG_RUNTIME_DIR` on Linux, the
/// per-user `$TMPDIR` on macOS).
///
/// # Errors
/// Fails when that variable is not set to an absolute path.
pub fn socket_path() -> io::Result<PathBuf> {
    let var = if cfg!(target_os = "macos") {
        "TMPDIR"
    } else {
        "XDG_RUNTIME_DIR"
    };
    let dir = std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{var} is not set")))?;
    Ok(dir.join("construct").join(SOCKET_NAME))
}

#[cfg(unix)]
pub use unix::{query_status, serve, Server};

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

    use super::{status_response, unknown_command_response};

    /// Mode of the `construct` runtime directory: the user only.
    const DIR_MODE: u32 = 0o700;
    /// Mode of the socket file: the user only.
    const SOCKET_MODE: u32 = 0o600;
    /// Pause after a failed `accept`, so a persistent error (no free file
    /// descriptors) does not spin the CPU.
    const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);
    /// How long one `status` call may take in total, connect included.
    const QUERY_TIMEOUT: Duration = Duration::from_secs(5);

    pub struct Server {
        listener: UnixListener,
        path: PathBuf,
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
            if !dir.exists() {
                fs::DirBuilder::new()
                    .recursive(true)
                    .mode(DIR_MODE)
                    .create(dir)?;
            }
            fs::set_permissions(dir, fs::Permissions::from_mode(DIR_MODE))?;
            remove_stale_socket(path)?;
            let listener = UnixListener::bind(path)?;
            if let Err(e) = fs::set_permissions(path, fs::Permissions::from_mode(SOCKET_MODE)) {
                let _ = fs::remove_file(path);
                return Err(e);
            }
            Ok(Self {
                listener,
                path: path.to_path_buf(),
            })
        }

        #[must_use]
        pub fn path(&self) -> &Path {
            &self.path
        }

        /// Accept connections forever, one thread each. An accept error is logged and
        /// retried.
        pub fn serve(&self) {
            for conn in self.listener.incoming() {
                match conn {
                    Ok(stream) => {
                        thread::spawn(move || {
                            if let Err(e) = handle(stream) {
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

    /// Bind and serve on `path`.
    ///
    /// # Errors
    /// Fails when the socket cannot be bound.
    pub fn serve(path: &Path) -> io::Result<()> {
        let server = Server::bind(path)?;
        eprintln!(
            "construct-agent {} listening on {}",
            env!("CARGO_PKG_VERSION"),
            server.path().display()
        );
        server.serve();
        Ok(())
    }

    fn handle(stream: UnixStream) -> io::Result<()> {
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        let mut line = String::new();
        BufReader::new(io::Read::take(&stream, 1024)).read_line(&mut line)?;
        let mut stream = stream;
        let reply = match line.trim() {
            "status" => status_response(std::process::id()),
            other => unknown_command_response(other),
        };
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
