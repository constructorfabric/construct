//! The embedded PostgreSQL, through the real agent binary. These tests unpack and start
//! PostgreSQL, so they are ignored by default: `cargo test -- --ignored`.

#![cfg(target_os = "linux")]
// The helpers below are test code: a failed `unwrap` is a failed test.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use construct_agent::ipc::query_status;

/// A spawned agent, killed when the test ends, also when it fails.
struct Agent(Child);

impl Agent {
    fn id(&self) -> u32 {
        self.0.id()
    }

    fn kill(&mut self) {
        self.0.kill().unwrap();
        self.0.wait().unwrap();
    }

    /// SIGTERM, then wait for the exit.
    fn terminate(&mut self) {
        Command::new("kill")
            .arg(self.id().to_string())
            .status()
            .unwrap();
        self.0.wait().unwrap();
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A scratch user environment. The names are short: a socket path is limited to
/// about 108 bytes.
struct Stand {
    root: PathBuf,
}

impl Stand {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("cb-{name}-{}", std::process::id()));
        std::fs::create_dir_all(root.join("run")).unwrap();
        std::fs::set_permissions(
            root.join("run"),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        Self { root }
    }

    fn agent(&self) -> Agent {
        Agent(self.command().spawn().unwrap())
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_construct-agent"));
        command
            .arg("run")
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    fn agent_socket(&self) -> PathBuf {
        self.root.join("run/construct/agent.sock")
    }

    fn pg_socket_dir(&self) -> PathBuf {
        self.root.join("run/construct/pg")
    }

    fn data_dir(&self) -> PathBuf {
        self.root.join("data/construct/postgres")
    }

    fn psql_path(&self) -> PathBuf {
        let bins = self.root.join("data/construct/postgres-bin");
        let version = std::fs::read_dir(bins)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.is_dir())
            .unwrap();
        version.join("bin/psql")
    }

    /// Run one SQL statement as `postgres` over the socket; returns the output.
    fn sql(&self, statement: &str) -> String {
        let out = Command::new(self.psql_path())
            .args(["-h"])
            .arg(self.pg_socket_dir())
            .args(["-U", "postgres", "-Atc", statement])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "psql failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    /// Wait until `status` reports `database: ready`.
    fn wait_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(180);
        while Instant::now() < deadline {
            if let Ok(line) = query_status(&self.agent_socket()) {
                let v: serde_json::Value = serde_json::from_str(&line).unwrap();
                assert_ne!(v["database"], "failed", "{line}");
                if v["database"] == "ready" {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        panic!("the database did not become ready");
    }

    fn postmaster_pid(&self) -> Option<u32> {
        let text = std::fs::read_to_string(self.data_dir().join("postmaster.pid")).ok()?;
        text.lines().next()?.parse().ok()
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        // Do not leave a PostgreSQL of a failed test running.
        if let Some(pid) = self.postmaster_pid() {
            let _ = Command::new("kill").arg(pid.to_string()).status();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn wait_gone(pid: u32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if !alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

#[test]
#[ignore = "starts PostgreSQL; run with --ignored"]
fn starts_with_pgvector_on_a_socket_only_and_stops_with_the_agent() {
    let stand = Stand::new("life");
    let mut agent = stand.agent();

    // `status` answers at once, while the first run unpacks PostgreSQL.
    let started = Instant::now();
    let deadline = started + Duration::from_secs(10);
    let first = loop {
        if let Ok(line) = query_status(&stand.agent_socket()) {
            break line;
        }
        assert!(Instant::now() < deadline, "no status within 10 s");
        std::thread::sleep(Duration::from_millis(50));
    };
    let v: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(v["status"], "running");
    assert!(
        v["database"] == "starting" || v["database"] == "ready",
        "{first}"
    );

    stand.wait_ready();
    assert_eq!(stand.sql("select current_user"), "postgres");
    stand.sql("create extension vector");
    let version = stand.sql("select extversion from pg_extension where extname='vector'");
    assert_eq!(version, "0.8.7");
    assert_eq!(
        stand.sql("select '[1,2,3]'::vector <-> '[1,2,4]'::vector"),
        "1"
    );

    // No TCP: PostgreSQL listens on nothing, and the socket is in the user-only directory.
    assert_eq!(stand.sql("show listen_addresses"), "");
    assert_eq!(stand.sql("show fsync"), "on");
    assert!(stand.pg_socket_dir().join(".s.PGSQL.5432").exists());
    let pid = stand.postmaster_pid().unwrap();
    let listening = Command::new("ss").args(["-ltnp"]).output().unwrap();
    let listening = String::from_utf8_lossy(&listening.stdout);
    assert!(
        !listening
            .lines()
            .any(|l| l.contains(&format!("pid={pid},"))),
        "{listening}"
    );

    // Stopping the agent stops PostgreSQL.
    agent.terminate();
    assert!(wait_gone(pid), "postgres {pid} is still running");
    assert!(stand.postmaster_pid().is_none());
}

#[test]
#[ignore = "starts PostgreSQL; run with --ignored"]
fn data_survives_kill_9_of_the_agent_and_a_restart() {
    let stand = Stand::new("kill");
    let mut agent = stand.agent();
    stand.wait_ready();
    stand.sql("create table kept(v int)");
    stand.sql("insert into kept values (42)");
    let old_pid = stand.postmaster_pid().unwrap();

    // SIGKILL gives the agent no chance to stop PostgreSQL; outside systemd it keeps running.
    agent.kill();
    assert!(alive(old_pid), "the old postmaster died with the agent");

    let mut agent = stand.agent();
    stand.wait_ready();
    assert_eq!(stand.sql("select v from kept"), "42");
    // The new agent stopped the old PostgreSQL and started its own.
    assert_ne!(stand.postmaster_pid().unwrap(), old_pid);
    assert!(!alive(old_pid));

    agent.terminate();
}

#[test]
#[ignore = "starts PostgreSQL; run with --ignored"]
fn a_second_agent_leaves_the_first_ones_database_alone() {
    let stand = Stand::new("two");
    let mut first = stand.agent();
    stand.wait_ready();
    let pid = stand.postmaster_pid().unwrap();

    let status = stand.command().arg("run").status().unwrap();
    assert!(!status.success(), "a second agent must not start");

    assert_eq!(stand.postmaster_pid(), Some(pid));
    assert!(alive(pid));
    assert_eq!(stand.sql("select 1"), "1");
    let v: serde_json::Value =
        serde_json::from_str(&query_status(&stand.agent_socket()).unwrap()).unwrap();
    assert_eq!(v["database"], "ready");
    first.terminate();
}

#[test]
#[ignore = "starts PostgreSQL; run with --ignored"]
fn status_turns_failed_when_the_postmaster_is_killed() {
    let stand = Stand::new("gone");
    let mut agent = stand.agent();
    stand.wait_ready();
    let pid = stand.postmaster_pid().unwrap();
    Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let v: serde_json::Value =
            serde_json::from_str(&query_status(&stand.agent_socket()).unwrap()).unwrap();
        if v["database"] == "failed" {
            break;
        }
        assert!(Instant::now() < deadline, "status stayed {}", v["database"]);
        std::thread::sleep(Duration::from_millis(500));
    }
    agent.terminate();
}
