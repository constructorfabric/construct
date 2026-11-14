//! `construct-agent ensure-service` run as a program, with a fake `systemctl` first on
//! `PATH` and the service directory moved into a throw-away directory.

#![cfg(target_os = "linux")]
// The helpers sit outside `#[test]` functions, where `clippy.toml` does not allow `unwrap`.
#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A throw-away directory, removed on drop even when an assertion fails.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Put a fake `systemctl` that logs its calls into `<dir>/bin`; returns the log file.
fn fake_systemctl(dir: &Path) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = dir.join("systemctl.log");
    let script = bin.join("systemctl");
    std::fs::write(
        &script,
        format!("#!/bin/sh\necho \"$*\" >> '{}'\n", log.display()),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    log
}

/// `PATH` with the fake `systemctl` of `dir` first.
fn path_with_fake(dir: &Path) -> String {
    format!(
        "{}:{}",
        dir.join("bin").display(),
        std::env::var("PATH").unwrap()
    )
}

/// Run the agent copy at `agent` with `ensure-service` and return what it printed.
fn ensure_service(dir: &Path, agent: &Path) -> String {
    let out = Command::new(agent)
        .arg("ensure-service")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("PATH", path_with_fake(dir))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// What the fake `systemctl` was called with since the last read, one call per line.
fn systemctl_calls(log: &Path) -> Vec<String> {
    let calls = std::fs::read_to_string(log).unwrap_or_default();
    let _ = std::fs::remove_file(log);
    calls.lines().map(str::to_string).collect()
}

#[test]
fn ensure_service_registers_once_and_restarts_after_the_binary_is_replaced() {
    let dir = Dir(std::env::temp_dir().join(format!("construct-agent-cli-{}", std::process::id())));
    let log = fake_systemctl(&dir.0);
    // The agent is copied, so the test can replace its binary.
    let agent = dir.0.join("construct-agent");
    std::fs::copy(env!("CARGO_BIN_EXE_construct-agent"), &agent).unwrap();

    let install = [
        "--user daemon-reload",
        "--user enable --now construct-agent.service",
        "--user restart construct-agent.service",
    ];
    let enable = &install[..2];

    assert!(ensure_service(&dir.0, &agent).contains("construct-agent is registered"));
    assert_eq!(systemctl_calls(&log), install);
    let unit = dir.0.join("config/systemd/user/construct-agent.service");
    assert!(std::fs::read_to_string(&unit)
        .unwrap()
        .contains(&agent.display().to_string()));

    ensure_service(&dir.0, &agent);
    assert_eq!(
        systemctl_calls(&log),
        enable,
        "the second start leaves the running agent alone"
    );

    // An upgrade in place: the same path, another build (an ELF file ignores trailing bytes).
    std::fs::OpenOptions::new()
        .append(true)
        .open(&agent)
        .unwrap()
        .write_all(b"another build")
        .unwrap();
    ensure_service(&dir.0, &agent);
    assert_eq!(systemctl_calls(&log), install);
}

#[test]
fn a_relative_home_is_refused_before_anything_is_registered() {
    let dir =
        Dir(std::env::temp_dir().join(format!("construct-agent-home-{}", std::process::id())));
    let log = fake_systemctl(&dir.0);
    let out = Command::new(env!("CARGO_BIN_EXE_construct-agent"))
        .arg("ensure-service")
        .current_dir(&dir.0)
        .env_remove("XDG_CONFIG_HOME")
        .env("HOME", "relative/home")
        .env("PATH", path_with_fake(&dir.0))
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("HOME is not set to an absolute path"),
        "{stderr}"
    );
    assert!(systemctl_calls(&log).is_empty());
    assert!(!dir.0.join("relative").exists());
}
