//! The embedded PostgreSQL, through the real agent binary. The tests that unpack and start
//! PostgreSQL are ignored by default: `cargo test -- --ignored`.

#![cfg(target_os = "linux")]
// The helpers below are test code: a failed `unwrap` is a failed test.
#![allow(clippy::unwrap_used)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use construct_agent::database::running_postmaster;
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

    /// Send SIGTERM.
    fn signal(&self) {
        Command::new("kill")
            .arg(self.id().to_string())
            .status()
            .unwrap();
    }

    /// SIGTERM, then wait for the exit.
    fn terminate(&mut self) {
        self.signal();
        self.0.wait().unwrap();
    }

    /// Wait for the agent to exit on its own, for at most `limit`.
    fn wait_exit(&mut self, limit: Duration) -> std::process::ExitStatus {
        let deadline = Instant::now() + limit;
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "the agent kept running");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Stops the process `pid` when dropped, so a failed assertion leaves no postmaster behind.
struct KillOnDrop(u32);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = Command::new("kill").arg(self.0.to_string()).status();
    }
}

/// Wait until the agent socket `socket` reports `database: <want>`.
fn wait_for_database(socket: &Path, want: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(line) = query_status(socket) {
            let v: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(v["status"], "running");
            if v["database"] == want {
                return;
            }
        }
        assert!(Instant::now() < deadline, "status never showed {want}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The PID in `<data_dir>/postmaster.pid`, if there is one.
fn postmaster_pid_in(data_dir: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(data_dir.join("postmaster.pid")).ok()?;
    text.lines().next()?.parse().ok()
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
        Self::command_in(&self.root.join("run"), &self.root.join("data"))
    }

    /// The agent with the given runtime directory and data home.
    fn command_in(runtime: &Path, data_home: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_construct-agent"));
        command
            .arg("run")
            .env("XDG_DATA_HOME", data_home)
            .env("XDG_RUNTIME_DIR", runtime)
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
        postmaster_pid_in(&self.data_dir())
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
    // pgvector is built for Linux x86_64 only; elsewhere the database runs without it.
    if cfg!(target_arch = "x86_64") {
        stand.sql("create extension vector");
        let version = stand.sql("select extversion from pg_extension where extname='vector'");
        assert_eq!(
            version,
            stand.sql("select default_version from pg_available_extensions where name='vector'")
        );
        assert_eq!(
            stand.sql("select '[1,2,3]'::vector <-> '[1,2,4]'::vector"),
            "1"
        );
    }

    // No TCP: PostgreSQL listens on nothing, and the socket is in the user-only directory.
    assert_eq!(stand.sql("show listen_addresses"), "");
    assert_eq!(stand.sql("show fsync"), "on");
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&stand.pg_socket_dir()), 0o700);
    assert_eq!(mode(&stand.pg_socket_dir().join(".s.PGSQL.5432")), 0o700);
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

    let status = stand.command().status().unwrap();
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
fn an_agent_with_another_runtime_directory_leaves_the_same_data_alone() {
    let stand = Stand::new("lock");
    let mut first = stand.agent();
    stand.wait_ready();
    let pid = stand.postmaster_pid().unwrap();

    // Its own agent socket is free, so only the lock on the data directory refuses its database.
    let other = stand.root.join("run2");
    std::fs::create_dir_all(&other).unwrap();
    let mut second = Agent(
        Stand::command_in(&other, &stand.root.join("data"))
            .spawn()
            .unwrap(),
    );
    wait_for_database(&other.join("construct/agent.sock"), "failed");
    second.terminate();

    assert_eq!(stand.postmaster_pid(), Some(pid));
    assert_eq!(stand.sql("select 1"), "1");
    first.terminate();
}

#[test]
fn a_failed_start_shows_failed_while_the_agent_keeps_running() {
    let stand = Stand::new("fail");
    // The data home is a file, so the data directory cannot be created.
    let data_home = stand.root.join("not-a-directory");
    std::fs::write(&data_home, "").unwrap();
    let mut agent = Agent(
        Stand::command_in(&stand.root.join("run"), &data_home)
            .spawn()
            .unwrap(),
    );

    wait_for_database(&stand.agent_socket(), "failed");
    assert!(agent.0.try_wait().unwrap().is_none(), "the agent exited");
    agent.terminate();
}

#[test]
fn a_signal_while_the_database_is_starting_stops_the_agent_cleanly() {
    let stand = Stand::new("sig");
    // `XDG_DATA_HOME` is not absolute, so the data goes under `HOME`.
    let home = stand.root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let mut command = Stand::command_in(&stand.root.join("run"), Path::new("data"));
    command.env("HOME", &home);
    let mut agent = Agent(command.spawn().unwrap());

    // Signal as soon as PostgreSQL has a postmaster, while the agent is still starting. The
    // window in which only `stop_leftover` can stop it lasts milliseconds, so this checks the
    // outcome (a clean exit and no postmaster left), not that branch. A failed assertion must
    // not leave the postmaster behind.
    let data_dir = home.join(".local/share/construct/postgres");
    let deadline = Instant::now() + Duration::from_secs(60);
    let pid = loop {
        if let Some(pid) = postmaster_pid_in(&data_dir) {
            break pid;
        }
        assert!(Instant::now() < deadline, "PostgreSQL never started");
        std::thread::sleep(Duration::from_millis(5));
    };
    let _guard = KillOnDrop(pid);
    agent.signal();
    let status = agent.wait_exit(Duration::from_secs(30));
    assert!(status.success(), "{status}");
    assert!(wait_gone(pid), "postgres {pid} is still running");
}

#[test]
#[ignore = "starts PostgreSQL; run with --ignored"]
fn the_agent_exits_with_an_error_when_the_postmaster_is_killed() {
    let stand = Stand::new("gone");
    let mut agent = stand.agent();
    stand.wait_ready();
    let pid = stand.postmaster_pid().unwrap();
    Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .unwrap();

    // The service manager starts the agent again after this exit, and the database with it.
    let status = agent.wait_exit(Duration::from_secs(30));
    assert!(!status.success());
}

/// A stand-in whose command line is `<dir>/postgres -D <data_dir>`, as a real postmaster's.
fn stand_in_postmaster(dir: &Path, data_dir: &Path) -> Agent {
    use std::os::unix::process::CommandExt;
    Agent(
        // `sh -c <script> <operands>` ignores the operands, so the command line can carry
        // `-D <dir>`. Two commands, so that bash does not exec the lone `sleep` over the line.
        Command::new("sh")
            .arg0(dir.join("postgres"))
            .args(["-c", "sleep 60; :", "stand-in", "-D"])
            .arg(data_dir)
            .spawn()
            .unwrap(),
    )
}

#[test]
fn only_a_postgres_started_on_the_data_directory_is_a_running_postmaster() {
    let root = std::env::temp_dir().join(format!("cb-pm-{}", std::process::id()));
    let data_dir = root.join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    let write_pid = |pid: u32| {
        std::fs::write(data_dir.join("postmaster.pid"), format!("{pid}\n")).unwrap();
    };

    // This test process is alive but is not a postgres.
    write_pid(std::process::id());
    assert_eq!(running_postmaster(&data_dir), None);

    // A process that looks like `postgres -D <data_dir>` is found.
    let same = stand_in_postmaster(&root, &data_dir);
    write_pid(same.id());
    assert_eq!(running_postmaster(&data_dir), Some(same.id()));
    // The same process, asked about another data directory, is left alone.
    assert_eq!(running_postmaster(&root.join("other")), None);
    drop(same);

    // No such process, and no pid file.
    write_pid(4_194_300);
    assert_eq!(running_postmaster(&data_dir), None);
    std::fs::remove_file(data_dir.join("postmaster.pid")).unwrap();
    assert_eq!(running_postmaster(&data_dir), None);
    let _ = std::fs::remove_dir_all(root);
}
