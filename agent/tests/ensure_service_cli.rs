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

/// Run the agent copy at `agent` with `ensure-service` and return what it printed.
fn ensure_service(dir: &Path, agent: &Path) -> String {
    let out = Command::new(agent)
        .arg("ensure-service")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env(
            "PATH",
            format!(
                "{}:{}",
                dir.join("bin").display(),
                std::env::var("PATH").unwrap()
            ),
        )
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
    let bin = dir.0.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = dir.0.join("systemctl.log");
    let script = bin.join("systemctl");
    std::fs::write(
        &script,
        format!("#!/bin/sh\necho \"$*\" >> '{}'\n", log.display()),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
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
