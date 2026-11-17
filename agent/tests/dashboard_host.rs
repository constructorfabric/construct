//! The real agent: the dashboard types are in the host's types registry once it has started, and
//! the default dashboard is in the workspace. The test unpacks and starts PostgreSQL, so it is
//! ignored by default: `cargo test -- --ignored`.

#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use construct_agent::dashboard::{ACTIVITY_FILE, ACTIVITY_TYPE, BASE_TYPE, validate_activity};
use construct_agent::ipc::request;
use serde_json::Value;

/// A started agent, stopped when it goes out of scope, and the directory it lives in.
struct Agent {
    child: Child,
    root: PathBuf,
}

impl Agent {
    fn start() -> Self {
        let root = std::env::temp_dir().join(format!("cb-dashboard-host-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("run")).unwrap();
        std::fs::create_dir_all(root.join("home")).unwrap();
        std::fs::set_permissions(
            root.join("run"),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_construct-agent"))
            .arg("run")
            .env("HOME", root.join("home"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_RUNTIME_DIR", root.join("run"))
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Self { child, root }
    }

    fn wait_ready(&self) {
        let socket = self.root.join("run/construct/agent.sock");
        let deadline = Instant::now() + Duration::from_secs(300);
        while Instant::now() < deadline {
            if let Ok(reply) = request(&socket, "status", Duration::from_secs(5))
                && let Ok(status) = serde_json::from_str::<Value>(&reply)
                && status["construct"] == "ready"
            {
                return;
            }
            thread::sleep(Duration::from_millis(200));
        }
        panic!("timed out waiting for the host");
    }

    /// The types registry's answer for `type_id`: the status and the JSON.
    fn registry_entity(&self, type_id: &str) -> (u16, Value) {
        let file = self.root.join("run/construct/connection.json");
        let connection: Value =
            serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .proxy(None)
            .http_status_as_error(false)
            .build()
            .into();
        let mut response = agent
            .get(&format!(
                "http://127.0.0.1:{}/types-registry/v1/entities/{type_id}",
                connection["port"]
            ))
            .header(
                "Authorization",
                format!("Bearer {}", connection["token"].as_str().unwrap()),
            )
            .call()
            .unwrap();
        let body = response.body_mut().read_to_string().unwrap();
        (
            response.status().as_u16(),
            serde_json::from_str(&body).unwrap_or(Value::Null),
        )
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .arg(self.child.id().to_string())
            .status();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
#[ignore = "starts PostgreSQL and the Construct host; run with --ignored"]
fn the_started_host_has_the_dashboard_types_and_the_workspace_the_default_dashboard() {
    let agent = Agent::start();
    agent.wait_ready();

    for type_id in [BASE_TYPE, ACTIVITY_TYPE] {
        let (status, entity) = agent.registry_entity(type_id);
        assert_eq!(status, 200, "{type_id}: {entity}");
        assert!(entity.to_string().contains(type_id), "{entity}");
    }

    let file = agent
        .root
        .join("data/construct/workspace")
        .join(ACTIVITY_FILE);
    let document: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    validate_activity(&document).unwrap();
}
