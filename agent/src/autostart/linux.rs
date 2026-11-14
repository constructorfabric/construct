//! Linux: a systemd user service. systemd restarts the agent when it dies and
//! journald collects its logs (`journalctl --user -u construct-agent`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{Autostart, CommandRunner, SERVICE_NAME, binary_stamp, escape, render, with_lock};

const UNIT_TEMPLATE: &str = include_str!("../../templates/construct-agent.service");

pub struct SystemdUser {
    unit_dir: PathBuf,
    runner: Box<dyn CommandRunner>,
}

impl SystemdUser {
    #[must_use]
    pub fn new(unit_dir: PathBuf, runner: Box<dyn CommandRunner>) -> Self {
        Self { unit_dir, runner }
    }

    #[must_use]
    pub fn unit_path(&self) -> PathBuf {
        self.unit_dir.join(unit_name())
    }

    /// Holds the stamp of the binary the unit was last started with; written after
    /// the start went through.
    fn stamp_path(&self) -> PathBuf {
        self.unit_dir.join(format!("{}.stamp", unit_name()))
    }

    fn lock_path(&self) -> PathBuf {
        self.unit_dir.join(format!("{}.lock", unit_name()))
    }

    /// Write the unit, start it with `exe`, and only then record the stamp. A call that
    /// dies or fails on the way leaves the old stamp, so the next `ensure` starts over.
    fn install_locked(&self, exe: &Path) -> io::Result<()> {
        let stamp = binary_stamp(exe)?;
        fs::write(self.unit_path(), unit_file(exe))?;
        self.enable()?;
        // A running agent keeps its old binary after an upgrade; pick up the new one.
        self.runner
            .run("systemctl", &["--user", "restart", &unit_name()])?;
        fs::write(self.stamp_path(), stamp)
    }

    /// Reload systemd and enable the unit, starting it when it is not running.
    fn enable(&self) -> io::Result<()> {
        self.runner.run("systemctl", &["--user", "daemon-reload"])?;
        self.runner
            .run("systemctl", &["--user", "enable", "--now", &unit_name()])
    }
}

/// File and `systemctl` name of the unit.
fn unit_name() -> String {
    format!("{SERVICE_NAME}.service")
}

/// Content of the service file. The path is quoted so spaces are safe, and
/// `\`, `"`, `%`, `$` and line breaks are escaped; systemd expands `%` and `$`
/// in `ExecStart`.
#[must_use]
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
        with_lock(&self.lock_path(), || self.install_locked(exe))
    }

    fn ensure(&self, exe: &Path) -> io::Result<()> {
        fs::create_dir_all(&self.unit_dir)?;
        with_lock(&self.lock_path(), || {
            let stamp = binary_stamp(exe)?;
            let unit_matches =
                fs::read_to_string(self.unit_path()).is_ok_and(|current| current == unit_file(exe));
            let started_with_it =
                fs::read_to_string(self.stamp_path()).is_ok_and(|saved| saved == stamp);
            if unit_matches && started_with_it {
                self.enable()
            } else {
                self.install_locked(exe)
            }
        })
    }

    fn uninstall(&self) -> io::Result<()> {
        // Under the lock, like `install` and `ensure`; the lock file itself stays, since
        // deleting it would let a waiting call and a new call lock different files.
        fs::create_dir_all(&self.unit_dir)?;
        with_lock(&self.lock_path(), || {
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
            for file in [self.unit_path(), self.stamp_path()] {
                match fs::remove_file(file) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
            self.runner.run("systemctl", &["--user", "daemon-reload"])
        })
    }

    fn is_installed(&self) -> bool {
        self.unit_path().exists()
    }
}
