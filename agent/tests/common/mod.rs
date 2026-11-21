//! Shared by the autostart tests: a recording fake for `systemctl` and `launchctl`, and a
//! scenario that owns a throw-away directory with a fake agent binary. Shared by the chat
//! tests: the parts of a fake provider and a guard for a started agent.

// Helpers sit outside `#[test]` functions, where `clippy.toml` does not allow `unwrap`.
#![allow(clippy::unwrap_used, dead_code)]

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[cfg(unix)]
pub mod bodies;
#[cfg(unix)]
pub mod fake_host;
#[cfg(unix)]
pub mod hold;
#[cfg(unix)]
pub mod pg_stand;
#[cfg(unix)]
pub mod scripted;
#[cfg(unix)]
pub mod sessions;
#[cfg(unix)]
pub mod setup_rig;
#[cfg(unix)]
pub mod stand;
#[cfg(unix)]
pub mod tools;

use construct_agent::autostart::{CommandRunner, linux, macos};
use serde_json::{Value, json};

#[derive(Clone, Default)]
pub struct Recorder(
    pub Arc<Mutex<Vec<String>>>,
    Arc<Mutex<Vec<&'static str>>>,
    Arc<Mutex<LaunchJob>>,
);

/// What the fake `launchctl` remembers: after a `bootout` the job is gone until the next
/// `bootstrap`, but `print` still finds it for `lingering` more calls.
#[derive(Default)]
struct LaunchJob {
    gone: bool,
    lingering: usize,
}

impl Recorder {
    /// Makes every call whose text contains one of `needles` fail (`""` matches all).
    pub fn fail_on(&self, needles: &[&'static str]) {
        *self.1.lock().unwrap() = needles.to_vec();
    }

    /// After a `bootout`, `launchctl print` still finds the job `calls` more times.
    pub fn job_lingers_for(&self, calls: usize) {
        self.2.lock().unwrap().lingering = calls;
    }
}

impl CommandRunner for Recorder {
    fn run(&self, program: &str, args: &[&str]) -> io::Result<()> {
        let call = format!("{program} {}", args.join(" "));
        self.0.lock().unwrap().push(call.clone());
        if self
            .1
            .lock()
            .unwrap()
            .iter()
            .any(|needle| call.contains(needle))
        {
            return Err(io::Error::other(format!("{call} failed")));
        }
        let mut job = self.2.lock().unwrap();
        if call.starts_with("launchctl bootout") {
            job.gone = true;
        } else if call.starts_with("launchctl bootstrap") {
            job.gone = false;
        } else if call.starts_with("launchctl print") && job.gone {
            if job.lingering == 0 {
                return Err(io::Error::other(format!("{call} found no job")));
            }
            job.lingering -= 1;
        }
        Ok(())
    }
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A throw-away directory with a fake agent binary and a recorder, removed on drop even
/// when an assertion fails.
pub struct Scenario {
    pub dir: PathBuf,
    pub rec: Recorder,
    pub exe: PathBuf,
}

impl Scenario {
    pub fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "construct-agent-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("construct-agent");
        std::fs::write(&exe, b"binary one").unwrap();
        Self {
            dir,
            rec: Recorder::default(),
            exe,
        }
    }

    pub fn linux(&self) -> linux::SystemdUser {
        linux::SystemdUser::new(self.dir.join("systemd/user"), Box::new(self.rec.clone()))
    }

    pub fn macos(&self) -> macos::LaunchAgent {
        macos::LaunchAgent::new(self.dir.clone(), "1234".into(), Box::new(self.rec.clone()))
    }

    /// Like `macos`, but waits at most `wait` for the old job to go down.
    pub fn macos_waiting(&self, wait: Duration) -> macos::LaunchAgent {
        self.macos().with_unload_wait(wait)
    }

    /// Another copy of the agent at a different path.
    pub fn moved_binary(&self) -> PathBuf {
        let path = self.dir.join("moved/construct-agent");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"binary one").unwrap();
        path
    }

    /// Replace the binary at the same path with other content of the same length, as an
    /// upgrade in place does, and keep its modification time as `cp -p` would.
    pub fn replace_binary_keeping_mtime(&self) {
        let before = std::fs::metadata(&self.exe).unwrap().modified().unwrap();
        std::fs::write(&self.exe, b"binary two").unwrap();
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&self.exe)
            .unwrap();
        file.set_modified(before).unwrap();
    }

    /// Write the same bytes again: a newer modification time, the same content.
    pub fn touch_binary(&self) {
        let now = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&self.exe)
            .unwrap();
        file.set_modified(now).unwrap();
    }

    /// The recorded calls and forget them.
    pub fn take_calls(&self) -> Vec<String> {
        std::mem::take(&mut *self.rec.0.lock().unwrap())
    }

    /// Like `take_calls`, with the plist path of a `bootstrap` call cut off.
    pub fn take_macos_calls(&self) -> Vec<String> {
        self.take_calls()
            .into_iter()
            .map(|c| match c.find("bootstrap gui/1234 ") {
                Some(at) => c[..at + "bootstrap gui/1234 ".len()].to_string(),
                None => c,
            })
            .collect()
    }
}

impl Drop for Scenario {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub const LINUX_INSTALL: [&str; 3] = [
    "systemctl --user daemon-reload",
    "systemctl --user enable --now construct-agent.service",
    "systemctl --user restart construct-agent.service",
];
pub const LINUX_ENABLE: [&str; 2] = [
    "systemctl --user daemon-reload",
    "systemctl --user enable --now construct-agent.service",
];
pub const MACOS_INSTALL: [&str; 3] = [
    "launchctl bootout gui/1234/cf.construct.agent",
    MACOS_PRINT,
    "launchctl bootstrap gui/1234 ",
];
pub const MACOS_PRINT: &str = "launchctl print gui/1234/cf.construct.agent";

/// Kills the agent when the test ends, however it ends.
pub struct Agent(pub Child);

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Reads one request: its body and its `Authorization` header.
pub fn read_request(stream: &mut TcpStream) -> (String, String) {
    let request = read_http(stream);
    (
        String::from_utf8(request.body).unwrap(),
        request.authorization,
    )
}

/// One request as it came: the method, the path, the body bytes and the `Authorization` header.
pub struct Request {
    pub method: String,
    pub path: String,
    pub body: Vec<u8>,
    pub authorization: String,
}

pub fn read_http(stream: &mut TcpStream) -> Request {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    reader.read_line(&mut request_line).unwrap();
    let mut parts = request_line.split_whitespace();
    let (method, path) = (
        parts.next().unwrap_or_default().to_string(),
        parts.next().unwrap_or_default().to_string(),
    );
    let (mut length, mut authorization) = (0, String::new());
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let lower = line.to_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            length = v.trim().parse().unwrap();
        }
        if lower.starts_with("authorization:") {
            authorization = line.split_once(':').unwrap().1.trim().to_string();
        }
        if line.trim().is_empty() {
            break;
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    Request {
        method,
        path,
        body,
        authorization,
    }
}

pub fn head(stream: &mut TcpStream) {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    stream.flush().unwrap();
}

/// One chunk; `false` once the client has gone.
pub fn chunk(stream: &mut TcpStream, content: &str) -> bool {
    let data = json!({ "choices": [{ "delta": { "content": content } }] });
    write!(stream, "data: {data}\n\n")
        .and_then(|()| stream.flush())
        .is_ok()
}

pub fn done(stream: &mut TcpStream) {
    let _ = write!(stream, "data: [DONE]\n\n");
}

pub fn status(stream: &mut TcpStream, code: u16, body: &str) {
    write!(
        stream,
        "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
}

/// Keep the connection open and quiet until the test ends.
pub fn hold(stream: &TcpStream) {
    let _keep = stream.try_clone().unwrap();
    thread::sleep(Duration::from_secs(30));
}

/// A chunk of any shape.
pub fn raw_chunk(stream: &mut TcpStream, data: &Value) {
    write!(stream, "data: {data}\n\n").unwrap();
    stream.flush().unwrap();
}
