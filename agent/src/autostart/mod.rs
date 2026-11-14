//! Autostart: start the agent at login. Each supported platform implements
//! [`Autostart`]; on Windows `install` is refused until the agent socket exists there.

use std::io;
use std::path::Path;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::PathBuf;
use std::process::Command;

pub mod linux;
pub mod macos;
pub mod windows;

/// Service name of the systemd unit. The macOS label is `macos::LABEL`.
pub const SERVICE_NAME: &str = "construct-agent";

/// Register, start and remove the agent as a per-user login service.
pub trait Autostart {
    /// Register the agent to start at login and start it now.
    ///
    /// # Errors
    /// Fails when the registration cannot be written or the platform tool fails.
    fn install(&self, exe: &Path) -> io::Result<()>;
    /// Register the agent unless it is registered for `exe` already, so the app can
    /// call it at every start. A registration counts when it was written for this very
    /// binary (its path and its contents) and a start with it went through; otherwise
    /// this is [`Autostart::install`], which restarts the agent. A matching
    /// registration is never restarted: on Linux the unit is enabled again at every
    /// call, on macOS the job must be loaded and is loaded again when it is not.
    ///
    /// # Errors
    /// Same as [`Autostart::install`].
    fn ensure(&self, exe: &Path) -> io::Result<()>;
    /// Stop the agent and remove the registration.
    ///
    /// # Errors
    /// Fails when the platform tool or the file removal fails.
    fn uninstall(&self) -> io::Result<()>;
    /// Whether the registration exists.
    fn is_installed(&self) -> bool;
}

/// Runs external tools (`systemctl`, `launchctl`, `reg`); replaced in tests.
pub trait CommandRunner {
    /// # Errors
    /// Fails when the program cannot start or exits with an error.
    fn run(&self, program: &str, args: &[&str]) -> io::Result<()>;
}

pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[&str]) -> io::Result<()> {
        let out = Command::new(program).args(args).output()?;
        if out.status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "`{program} {}` failed ({}): {}",
                args.join(" "),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }
}

/// Fill `{{KEY}}` placeholders of a template file.
pub(crate) fn render(template: &str, values: &[(&str, &str)]) -> String {
    values
        .iter()
        .fold(template.to_string(), |text, (key, value)| {
            text.replace(&format!("{{{{{key}}}}}"), value)
        })
}

/// Escape `s` in one pass: each char found in `table` becomes its replacement.
pub(crate) fn escape(s: &str, table: &[(char, &str)]) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match table.iter().find(|(from, _)| *from == c) {
            Some((_, to)) => out.push_str(to),
            None => out.push(c),
        }
    }
    out
}

/// The uid from the output of `id -u`: the command must have succeeded and
/// printed a number.
///
/// # Errors
///
/// Fails when the command did not succeed or printed something other than a number.
pub fn parse_uid(success: bool, stdout: &str) -> io::Result<String> {
    let uid = stdout.trim();
    if success && !uid.is_empty() && uid.chars().all(|c| c.is_ascii_digit()) {
        Ok(uid.to_string())
    } else {
        Err(io::Error::other(format!("`id -u` gave no uid: {uid:?}")))
    }
}

/// A digest of the contents of the binary at `exe`, with its size and path. The registration
/// records it once a start went through, so a binary replaced at the same path (an
/// upgrade in place, whatever its timestamps) is registered again. The hash is not
/// cryptographic and may change with the Rust version: that only costs one restart.
pub(crate) fn binary_stamp(exe: &Path) -> io::Result<String> {
    use std::hash::Hasher;
    let bytes = std::fs::read(exe)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write(&bytes);
    Ok(format!(
        "{}-{:016x} {}",
        bytes.len(),
        hasher.finish(),
        exe.display()
    ))
}

/// Run `f` holding an exclusive lock on the file `lock`, so two `ensure-service`
/// calls (two app windows starting at once) never interleave.
pub(crate) fn with_lock<T>(lock: &Path, f: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(lock)?;
    file.lock()?;
    f()
}

/// The autostart module of the platform this binary was built for. This is
/// the one place that resolves platform paths from the environment. On Linux
/// an absolute `XDG_CONFIG_HOME` moves the service directory.
///
/// # Errors
/// Fails when the home directory cannot be found, `id -u` does not print a uid
/// (macOS), or the platform has no module.
pub fn current() -> io::Result<Box<dyn Autostart>> {
    #[cfg(target_os = "linux")]
    {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .map_or_else(|| home_dir().map(|h| h.join(".config")), Ok)?;
        Ok(Box::new(linux::SystemdUser::new(
            config.join("systemd/user"),
            Box::new(SystemRunner),
        )))
    }
    #[cfg(target_os = "macos")]
    {
        let out = Command::new("id").arg("-u").output()?;
        let uid = parse_uid(out.status.success(), &String::from_utf8_lossy(&out.stdout))?;
        Ok(Box::new(macos::LaunchAgent::new(
            home_dir()?,
            uid,
            Box::new(SystemRunner),
        )))
    }
    #[cfg(windows)]
    {
        Ok(Box::new(windows::RunKey::new(Box::new(SystemRunner))))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "start at login is not available on this platform yet",
        ))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn home_dir() -> io::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
}
