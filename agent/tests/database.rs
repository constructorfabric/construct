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

    /// Wait until `status` reports `construct` as `want` (`ready` or `failed`); any other
    /// final state fails the test.
    fn wait_construct(&self, want: &str) {
        let deadline = Instant::now() + Duration::from_secs(300);
        while Instant::now() < deadline {
            if let Ok(line) = query_status(&self.agent_socket()) {
                let v: serde_json::Value = serde_json::from_str(&line).unwrap();
                if v["construct"] == want {
                    return;
                }
                assert!(
                    v["construct"] == "starting",
                    "construct is {} while waiting for {want}: {line}",
                    v["construct"]
                );
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        panic!("construct did not become {want}");
    }

    fn token_file(&self) -> PathBuf {
        self.root.join("data/construct/host/token")
    }

    fn connection_file(&self) -> PathBuf {
        self.root.join("run/construct/connection.json")
    }

    /// The port and token the app reads.
    fn connection(&self) -> (u16, String) {
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(self.connection_file()).unwrap())
                .unwrap();
        (
            u16::try_from(v["port"].as_u64().unwrap()).unwrap(),
            v["token"].as_str().unwrap().to_string(),
        )
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

fn mode(path: &Path) -> u32 {
    std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(path).unwrap().permissions()) & 0o777
}

/// The status of `GET` on the static identity's profile, with `token` when given.
fn profile_status(port: u16, token: Option<&str>) -> u16 {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .proxy(None)
        .http_status_as_error(false)
        .build()
        .into();
    let subject = toolkit_security::constants::DEFAULT_SUBJECT_ID;
    let url = format!("http://127.0.0.1:{port}/construct/v1/subjects/{subject}/profile");
    let mut request = agent.get(&url);
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    request.call().unwrap().status().as_u16()
}

/// The TCP ports that process `pid` itself listens on: the listening sockets of
/// `/proc/net/tcp*` whose inode is one of the process's open files. Other processes, such as
/// the hosts of other stand tests running in parallel, do not count. Empty when the process
/// is gone.
fn listening_ports_of(pid: u32) -> std::collections::BTreeSet<u16> {
    let inodes: std::collections::BTreeSet<String> = std::fs::read_dir(format!("/proc/{pid}/fd"))
        .into_iter()
        .flatten()
        .filter_map(|entry| std::fs::read_link(entry.ok()?.path()).ok())
        .filter_map(|link| {
            let link = link.to_string_lossy().into_owned();
            Some(
                link.strip_prefix("socket:[")?
                    .strip_suffix(']')?
                    .to_string(),
            )
        })
        .collect();
    ["/proc/net/tcp", "/proc/net/tcp6"]
        .iter()
        .filter_map(|file| std::fs::read_to_string(file).ok())
        .flat_map(|text| {
            text.lines()
                .skip(1)
                .filter_map(|line| {
                    let fields: Vec<&str> = line.split_whitespace().collect();
                    // `st` 0A is LISTEN, the local address is `ADDR:PORT` in hex, and the
                    // inode is the tenth field.
                    if fields.get(3) != Some(&"0A") || !inodes.contains(*fields.get(9)?) {
                        return None;
                    }
                    u16::from_str_radix(fields.get(1)?.rsplit(':').next()?, 16).ok()
                })
                .collect::<Vec<_>>()
        })
        .collect()
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

#[test]
#[ignore = "starts PostgreSQL and the Construct host; run with --ignored"]
fn the_host_answers_only_with_the_token_and_leaves_nothing_behind() {
    let stand = Stand::new("host");
    // A file left by a host that is gone: the start must remove it, not wait to overwrite it.
    let construct_dir = stand.root.join("run/construct");
    std::fs::create_dir_all(&construct_dir).unwrap();
    std::fs::set_permissions(
        &construct_dir,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    std::fs::write(
        stand.connection_file(),
        r#"{"port":1,"token":"stale","url":"http://127.0.0.1:1"}"#,
    )
    .unwrap();
    let mut agent = stand.agent();

    // While the host starts, the stale file is gone.
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut database_ready = None;
    loop {
        let line = query_status(&stand.agent_socket()).unwrap_or_default();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
        assert_ne!(v["construct"], "failed", "{line}");
        if v["construct"] == "ready" {
            break;
        }
        if v["database"] == "ready" {
            let since = *database_ready.get_or_insert_with(Instant::now);
            if since.elapsed() > Duration::from_millis(1500) {
                assert!(
                    !stand.connection_file().exists(),
                    "the stale connection.json is still there while the host starts"
                );
            }
        }
        assert!(Instant::now() < deadline, "construct did not become ready");
        std::thread::sleep(Duration::from_millis(50));
    }

    // `connection.json` names the live host, and it and the token file are user-only.
    let (port, token) = stand.connection();
    assert_ne!(port, 1);
    assert_ne!(token, "stale");
    assert_eq!(mode(&stand.connection_file()), 0o600);
    assert_eq!(mode(&stand.token_file()), 0o600);
    assert_eq!(
        std::fs::read_to_string(stand.token_file()).unwrap().trim(),
        token
    );
    assert_eq!(token.len(), 64);
    assert_eq!(profile_status(port, Some(&token)), 200);
    assert_eq!(profile_status(port, None), 401);
    assert_eq!(profile_status(port, Some("wrong")), 401);

    // Stopping the agent stops the host first, then PostgreSQL: at no moment is PostgreSQL gone
    // while the host still answers. `connection.json` is removed.
    let postmaster = stand.postmaster_pid().unwrap();
    Command::new("kill")
        .arg(agent.id().to_string())
        .status()
        .unwrap();
    let mut host_outlived_database = false;
    while agent.0.try_wait().unwrap().is_none() {
        let database_gone = !alive(postmaster);
        let host_answers = std::net::TcpStream::connect(("127.0.0.1", port)).is_ok();
        host_outlived_database |= database_gone && host_answers;
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        !host_outlived_database,
        "PostgreSQL stopped under a serving host"
    );
    assert!(wait_gone(postmaster), "PostgreSQL still runs");
    assert!(
        !stand.connection_file().exists(),
        "connection.json is left behind"
    );
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", port)).is_err(),
        "the host port still answers"
    );

    // The next start reuses the token.
    let _agent = stand.agent();
    stand.wait_construct("ready");
    let (_, again) = stand.connection();
    assert_eq!(again, token);
}

#[test]
#[ignore = "starts PostgreSQL; run with --ignored"]
fn a_host_that_fails_leaves_the_database_up_and_no_connection_file() {
    let stand = Stand::new("hostfail");
    // A file where the host's directory belongs: the host cannot start.
    let parent = stand.root.join("data/construct");
    std::fs::create_dir_all(&parent).unwrap();
    std::fs::write(parent.join("host"), "in the way").unwrap();
    let _agent = stand.agent();

    stand.wait_construct("failed");
    stand.wait_ready();
    assert_eq!(stand.sql("select 1"), "1");
    assert!(!stand.connection_file().exists());
}

#[test]
#[ignore = "starts PostgreSQL and the Construct host; run with --ignored"]
fn a_connection_file_that_cannot_be_written_stops_the_host_it_began() {
    let stand = Stand::new("hostwrite");
    // The file is written through `connection.tmp`: a directory there makes the write fail
    // after the host already answers.
    let construct_dir = stand.root.join("run/construct");
    std::fs::create_dir_all(construct_dir.join("connection.tmp")).unwrap();
    std::fs::set_permissions(
        &construct_dir,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    let mut agent = stand.agent();

    stand.wait_construct("failed");
    stand.wait_ready();
    assert!(!stand.connection_file().exists());
    // The host that answered was stopped and awaited: the agent itself listens on no TCP port
    // (PostgreSQL is on a Unix socket).
    assert_eq!(
        listening_ports_of(agent.id()),
        std::collections::BTreeSet::new(),
        "the failed host still listens"
    );
    assert_eq!(stand.sql("select 1"), "1");
    agent.terminate();
}

#[test]
#[ignore = "starts PostgreSQL and the Construct host; run with --ignored"]
fn a_signal_while_the_host_starts_stops_it_before_the_database() {
    let stand = Stand::new("hostsig");
    let mut agent = stand.agent();
    stand.wait_ready();
    let postmaster = stand.postmaster_pid().unwrap();

    // The database is up and the host is still starting. While the agent exits, PostgreSQL
    // must not be gone while the agent still listens on a TCP port (a half-started host).
    Command::new("kill")
        .arg(agent.id().to_string())
        .status()
        .unwrap();
    let mut database_stopped_under_host = false;
    while agent.0.try_wait().unwrap().is_none() {
        database_stopped_under_host |=
            !alive(postmaster) && !listening_ports_of(agent.id()).is_empty();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        !database_stopped_under_host,
        "PostgreSQL stopped under a half-started host"
    );
    assert!(wait_gone(postmaster), "PostgreSQL still runs");
    assert!(!stand.connection_file().exists());
}
