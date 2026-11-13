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
pub fn status_response(pid: u32) -> String {
    serde_json::json!({
        "status": "running",
        "version": env!("CARGO_PKG_VERSION"),
        "pid": pid,
    })
    .to_string()
}

/// The JSON line the agent answers to anything else.
pub fn unknown_command_response(command: &str) -> String {
    serde_json::json!({ "error": "unknown command", "command": command }).to_string()
}

/// Socket path from the runtime directory (`$XDG_RUNTIME_DIR` on Linux, the
/// per-user `$TMPDIR` on macOS).
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
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
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

    pub struct Server {
        listener: UnixListener,
        path: PathBuf,
    }

    impl Server {
        /// Bind the socket, replacing a stale file left by a dead agent but
        /// refusing to replace a live one.
        pub fn bind(path: &Path) -> io::Result<Self> {
            let dir = path.parent().expect("socket path has a parent");
            if !dir.exists() {
                fs::DirBuilder::new()
                    .recursive(true)
                    .mode(DIR_MODE)
                    .create(dir)?;
            }
            fs::set_permissions(dir, fs::Permissions::from_mode(DIR_MODE))?;
            if path.exists() {
                if UnixStream::connect(path).is_ok() {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        format!("another agent is already listening on {}", path.display()),
                    ));
                }
                fs::remove_file(path)?;
            }
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

        pub fn path(&self) -> &Path {
            &self.path
        }

        /// Accept connections forever, one thread each.
        pub fn serve(&self) -> io::Result<()> {
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
            Ok(())
        }
    }

    /// Bind and serve on `path`.
    pub fn serve(path: &Path) -> io::Result<()> {
        let server = Server::bind(path)?;
        eprintln!(
            "construct-agent {} listening on {}",
            env!("CARGO_PKG_VERSION"),
            server.path().display()
        );
        server.serve()
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

    /// Ask the agent on `path` for its status; returns the JSON line.
    pub fn query_status(path: &Path) -> io::Result<String> {
        let mut stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
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
