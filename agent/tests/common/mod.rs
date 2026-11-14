//! Shared by the autostart tests: a recording fake for `systemctl` and `launchctl`, and a
//! scenario that owns a throw-away directory with a fake agent binary.

// Helpers sit outside `#[test]` functions, where `clippy.toml` does not allow `unwrap`.
#![allow(clippy::unwrap_used, dead_code)]

use std::cell::RefCell;
use std::io;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use construct_agent::autostart::{CommandRunner, linux, macos};

#[derive(Clone, Default)]
pub struct Recorder(pub Rc<RefCell<Vec<String>>>, Rc<RefCell<Vec<&'static str>>>);

impl Recorder {
    /// Makes every call whose text contains one of `needles` fail (`""` matches all).
    pub fn fail_on(&self, needles: &[&'static str]) {
        *self.1.borrow_mut() = needles.to_vec();
    }
}

impl CommandRunner for Recorder {
    fn run(&self, program: &str, args: &[&str]) -> io::Result<()> {
        let call = format!("{program} {}", args.join(" "));
        self.0.borrow_mut().push(call.clone());
        if self.1.borrow().iter().any(|needle| call.contains(needle)) {
            Err(io::Error::other(format!("{call} failed")))
        } else {
            Ok(())
        }
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
        std::mem::take(&mut *self.rec.0.borrow_mut())
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
pub const MACOS_INSTALL: [&str; 2] = [
    "launchctl bootout gui/1234/cf.construct.agent",
    "launchctl bootstrap gui/1234 ",
];
pub const MACOS_PRINT: &str = "launchctl print gui/1234/cf.construct.agent";
