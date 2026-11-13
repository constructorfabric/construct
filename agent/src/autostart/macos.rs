//! macOS: a LaunchAgent in `~/Library/LaunchAgents`. `KeepAlive` makes launchd
//! restart the agent when it dies. The tests check the generated file and the
//! `launchctl` calls against a recording fake.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{escape, render, Autostart, CommandRunner};

const PLIST_TEMPLATE: &str = include_str!("../../templates/cf.construct.agent.plist");

pub const LABEL: &str = "cf.construct.agent";

pub struct LaunchAgent {
    home: PathBuf,
    uid: String,
    runner: Box<dyn CommandRunner>,
}

impl LaunchAgent {
    pub fn new(home: PathBuf, uid: String, runner: Box<dyn CommandRunner>) -> Self {
        Self { home, uid, runner }
    }

    pub fn plist_path(&self) -> PathBuf {
        self.home
            .join("Library/LaunchAgents")
            .join(format!("{LABEL}.plist"))
    }

    fn log_path(&self) -> PathBuf {
        self.home.join("Library/Logs/construct-agent.log")
    }

    fn gui_domain(&self) -> String {
        format!("gui/{}", self.uid)
    }

    fn gui_domain_target(&self) -> String {
        format!("{}/{LABEL}", self.gui_domain())
    }
}

/// Content of the LaunchAgent file. Logs go to the standard `~/Library/Logs`.
pub fn plist(exe: &Path, log: &Path) -> String {
    let xml = |p: &Path| {
        escape(
            &p.display().to_string(),
            &[('&', "&amp;"), ('<', "&lt;"), ('>', "&gt;")],
        )
    };
    render(
        PLIST_TEMPLATE,
        &[("LABEL", LABEL), ("EXE", &xml(exe)), ("LOG", &xml(log))],
    )
}

impl Autostart for LaunchAgent {
    fn install(&self, exe: &Path) -> io::Result<()> {
        let path = self.plist_path();
        fs::create_dir_all(path.parent().expect("plist path has a parent"))?;
        fs::create_dir_all(self.log_path().parent().expect("log path has a parent"))?;
        fs::write(&path, plist(exe, &self.log_path()))?;
        let plist_arg = path.display().to_string();
        // Ignore "not loaded" on a first install; bootstrap reports the real error.
        let _ = self
            .runner
            .run("launchctl", &["bootout", &self.gui_domain_target()]);
        self.runner
            .run("launchctl", &["bootstrap", &self.gui_domain(), &plist_arg])
    }

    fn uninstall(&self) -> io::Result<()> {
        // Always try to stop it. Without a plist "not loaded" is expected and
        // ignored; with one, a failure is reported and the file stays, so the
        // command can be run again.
        let installed = self.is_installed();
        let stopped = self
            .runner
            .run("launchctl", &["bootout", &self.gui_domain_target()]);
        if installed {
            stopped?;
        }
        match fs::remove_file(self.plist_path()) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn is_installed(&self) -> bool {
        self.plist_path().exists()
    }
}
