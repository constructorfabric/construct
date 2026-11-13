//! Windows: a value in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//! Windows does not restart a Run-key program. `install` is refused until the
//! agent socket exists on Windows; the generated command stays, pinned by a
//! golden test (real-machine behaviour is verified separately).

use std::io;
use std::path::Path;

use super::{Autostart, CommandRunner, SERVICE_NAME};

pub const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

pub struct RunKey {
    runner: Box<dyn CommandRunner>,
}

impl RunKey {
    pub fn new(runner: Box<dyn CommandRunner>) -> Self {
        Self { runner }
    }
}

/// The command stored in the Run key value.
pub fn run_command(exe: &Path) -> String {
    format!("\"{}\" run", exe.display())
}

/// Arguments of `reg.exe` that write the Run key value.
pub fn reg_add_args(exe: &Path) -> Vec<String> {
    vec![
        "add".into(),
        RUN_KEY.into(),
        "/v".into(),
        SERVICE_NAME.into(),
        "/t".into(),
        "REG_SZ".into(),
        "/d".into(),
        run_command(exe),
        "/f".into(),
    ]
}

impl Autostart for RunKey {
    /// The Run key would start `run` at every login, and `run` has no Windows
    /// socket yet, so installing is refused until it does. `reg_add_args` is
    /// what it will write then.
    fn install(&self, _exe: &Path) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "install-service is not implemented on Windows yet: the agent socket is missing",
        ))
    }

    fn uninstall(&self) -> io::Result<()> {
        if self.is_installed() {
            self.runner
                .run("reg", &["delete", RUN_KEY, "/v", SERVICE_NAME, "/f"])?;
        }
        Ok(())
    }

    fn is_installed(&self) -> bool {
        self.runner
            .run("reg", &["query", RUN_KEY, "/v", SERVICE_NAME])
            .is_ok()
    }
}
