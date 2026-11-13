//! Linux: a systemd user service. systemd restarts the agent when it dies and
//! journald collects its logs (`journalctl --user -u construct-agent`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{escape, render, Autostart, CommandRunner, SERVICE_NAME};

const UNIT_TEMPLATE: &str = include_str!("../../templates/construct-agent.service");

pub struct SystemdUser {
    unit_dir: PathBuf,
    runner: Box<dyn CommandRunner>,
}

impl SystemdUser {
    pub fn new(unit_dir: PathBuf, runner: Box<dyn CommandRunner>) -> Self {
        Self { unit_dir, runner }
    }

    pub fn unit_path(&self) -> PathBuf {
        self.unit_dir.join(unit_name())
    }
}

/// File and `systemctl` name of the unit.
fn unit_name() -> String {
    format!("{SERVICE_NAME}.service")
}

/// Content of the service file. The path is quoted so spaces are safe, and
/// `\`, `"`, `%`, `$` and line breaks are escaped; systemd expands `%` and `$`
/// in `ExecStart`.
pub fn unit_file(exe: &Path) -> String {
    let exe = escape(
        &exe.display().to_string(),
        &[
            ('\\', "\\\\"),
            ('"', "\\\""),
            ('%', "%%"),
            ('$', "$$"),
            ('\n', "\\n"),
            ('\r', "\\r"),
        ],
    );
    render(UNIT_TEMPLATE, &[("EXE", &exe)])
}

impl Autostart for SystemdUser {
    fn install(&self, exe: &Path) -> io::Result<()> {
        fs::create_dir_all(&self.unit_dir)?;
        fs::write(self.unit_path(), unit_file(exe))?;
        let unit = unit_name();
        self.runner.run("systemctl", &["--user", "daemon-reload"])?;
        self.runner
            .run("systemctl", &["--user", "enable", "--now", &unit])?;
        // A running agent keeps its old binary after an upgrade; pick up the new one.
        self.runner.run("systemctl", &["--user", "restart", &unit])
    }

    fn uninstall(&self) -> io::Result<()> {
        let unit = unit_name();
        // Always try to stop it. Without a unit file "not loaded" is expected and
        // ignored; with one, a failure is reported and the file stays, so the
        // command can be run again.
        let installed = self.is_installed();
        let stopped = self.runner.run("systemctl", &["--user", "stop", &unit]);
        let disabled = self.runner.run("systemctl", &["--user", "disable", &unit]);
        if installed {
            stopped?;
            disabled?;
        }
        match fs::remove_file(self.unit_path()) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        self.runner.run("systemctl", &["--user", "daemon-reload"])
    }

    fn is_installed(&self) -> bool {
        self.unit_path().exists()
    }
}
