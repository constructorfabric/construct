//! Autostart: start the agent at login. Each supported platform implements
//! [`Autostart`]; Windows has none until the agent socket exists there.

use std::io;
use std::path::Path;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::PathBuf;
use std::process::Command;

pub mod linux;
pub mod macos;

/// Service name of the systemd unit. The macOS label is `macos::LABEL`.
pub const SERVICE_NAME: &str = "construct-agent";

/// Register, start and remove the agent as a per-user login service.
pub trait Autostart {
    /// Register the agent to start at login and start it now.
    fn install(&self, exe: &Path) -> io::Result<()>;
    /// Stop the agent and remove the registration.
    fn uninstall(&self) -> io::Result<()>;
    /// Whether the registration exists.
    fn is_installed(&self) -> bool;
}

/// Runs external tools (`systemctl`, `launchctl`); replaced in tests.
pub trait CommandRunner {
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
pub fn parse_uid(success: bool, stdout: &str) -> io::Result<String> {
    let uid = stdout.trim();
    if success && !uid.is_empty() && uid.chars().all(|c| c.is_ascii_digit()) {
        Ok(uid.to_string())
    } else {
        Err(io::Error::other(format!("`id -u` gave no uid: {uid:?}")))
    }
}

/// The autostart module of the platform this binary was built for. This is
/// the one place that resolves platform paths from the environment.
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
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
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
