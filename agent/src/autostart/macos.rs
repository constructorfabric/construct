//! macOS: a `LaunchAgent` in `~/Library/LaunchAgents`. `KeepAlive` makes launchd
//! restart the agent when it dies. The tests check the generated file and the
//! `launchctl` calls against a recording fake.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use super::{Autostart, CommandRunner, binary_stamp, escape, render, with_lock};

const PLIST_TEMPLATE: &str = include_str!("../../templates/cf.construct.agent.plist");

pub const LABEL: &str = "cf.construct.agent";

/// The longest `reload` waits for the old job to go down after `bootout`. The agent stops its
/// database first, which takes a few seconds. The app gives `ensure-service` 30 seconds.
const UNLOAD_WAIT: Duration = Duration::from_secs(25);
const UNLOAD_PAUSE: Duration = Duration::from_millis(100);

pub struct LaunchAgent {
    home: PathBuf,
    uid: String,
    runner: Box<dyn CommandRunner>,
    unload_wait: Duration,
}

impl LaunchAgent {
    #[must_use]
    pub fn new(home: PathBuf, uid: String, runner: Box<dyn CommandRunner>) -> Self {
        Self {
            home,
            uid,
            runner,
            unload_wait: UNLOAD_WAIT,
        }
    }

    /// Changes the longest wait for the old job to go down.
    #[must_use]
    pub fn with_unload_wait(mut self, wait: Duration) -> Self {
        self.unload_wait = wait;
        self
    }

    #[must_use]
    pub fn plist_path(&self) -> PathBuf {
        self.launch_agents_dir().join(format!("{LABEL}.plist"))
    }

    fn launch_agents_dir(&self) -> PathBuf {
        self.home.join("Library/LaunchAgents")
    }

    fn log_dir(&self) -> PathBuf {
        self.home.join("Library/Logs")
    }

    fn log_path(&self) -> PathBuf {
        self.log_dir().join("construct-agent.log")
    }

    fn gui_domain(&self) -> String {
        format!("gui/{}", self.uid)
    }

    fn gui_domain_target(&self) -> String {
        format!("{}/{LABEL}", self.gui_domain())
    }

    /// Whether launchd has the job loaded.
    fn is_loaded(&self) -> bool {
        self.runner
            .run("launchctl", &["print", &self.gui_domain_target()])
            .is_ok()
    }

    /// Holds the stamp of the binary the job was last loaded with; written after the
    /// load went through.
    fn stamp_path(&self) -> PathBuf {
        self.launch_agents_dir()
            .join(format!("{LABEL}.plist.stamp"))
    }

    fn lock_path(&self) -> PathBuf {
        self.launch_agents_dir().join(format!("{LABEL}.plist.lock"))
    }

    /// Write the plist, load the job from it, and only then record the stamp. A call
    /// that dies or fails on the way leaves the old stamp, so the next `ensure` starts over.
    fn install_locked(&self, exe: &Path) -> io::Result<()> {
        let stamp = binary_stamp(exe)?;
        fs::create_dir_all(self.log_dir())?;
        fs::write(self.plist_path(), plist(exe, &self.log_path()))?;
        self.reload()?;
        fs::write(self.stamp_path(), stamp)
    }

    /// Unload the job and load it again from the file. "Not loaded" is expected on a
    /// first install. A failed `bootout` of a job that is still loaded is an error:
    /// bootstrapping over it would keep the old program. After a `bootout` that went
    /// through, the old job is still going down for a while, and launchd refuses a
    /// `bootstrap` until it is gone.
    fn reload(&self) -> io::Result<()> {
        match self
            .runner
            .run("launchctl", &["bootout", &self.gui_domain_target()])
        {
            Ok(()) => {
                self.wait_until_unloaded()?;
                self.bootstrap()
            }
            Err(e) if self.is_loaded() => Err(e),
            Err(_) => self.bootstrap(),
        }
    }

    fn wait_until_unloaded(&self) -> io::Result<()> {
        let deadline = Instant::now() + self.unload_wait;
        while self.is_loaded() {
            if Instant::now() >= deadline {
                return Err(io::Error::other(
                    "the old job is still loaded after `launchctl bootout`",
                ));
            }
            thread::sleep(UNLOAD_PAUSE);
        }
        Ok(())
    }

    fn bootstrap(&self) -> io::Result<()> {
        let plist_arg = self.plist_path().display().to_string();
        self.runner
            .run("launchctl", &["bootstrap", &self.gui_domain(), &plist_arg])
    }
}

/// Content of the `LaunchAgent` file. Logs go to the standard `~/Library/Logs`.
#[must_use]
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
        fs::create_dir_all(self.launch_agents_dir())?;
        with_lock(&self.lock_path(), || self.install_locked(exe))
    }

    fn ensure(&self, exe: &Path) -> io::Result<()> {
        fs::create_dir_all(self.launch_agents_dir())?;
        with_lock(&self.lock_path(), || {
            let stamp = binary_stamp(exe)?;
            let plist_matches = fs::read_to_string(self.plist_path())
                .is_ok_and(|current| current == plist(exe, &self.log_path()));
            let loaded_with_it =
                fs::read_to_string(self.stamp_path()).is_ok_and(|saved| saved == stamp);
            if !(plist_matches && loaded_with_it) {
                self.install_locked(exe)
            } else if self.is_loaded() {
                Ok(())
            } else {
                self.bootstrap()
            }
        })
    }

    fn uninstall(&self) -> io::Result<()> {
        // Under the lock, like `install` and `ensure`; the lock file itself stays, since
        // deleting it would let a waiting call and a new call lock different files.
        fs::create_dir_all(self.launch_agents_dir())?;
        with_lock(&self.lock_path(), || {
            // Always try to stop it. A failed `bootout` is an error only while the
            // job is still loaded: then the file stays, so the command can be run
            // again. A job that is not loaded is already stopped.
            let target = self.gui_domain_target();
            if let Err(e) = self.runner.run("launchctl", &["bootout", &target])
                && self.is_loaded()
            {
                return Err(e);
            }
            for file in [self.plist_path(), self.stamp_path()] {
                match fs::remove_file(file) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(())
        })
    }

    fn is_installed(&self) -> bool {
        self.plist_path().exists()
    }
}
