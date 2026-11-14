//! The `construct-agent run` binary follows the logs: its `status` shows the turns it read.

#![cfg(unix)]

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use construct_agent::ipc::query_status;

/// Kills the agent when the test ends, however it ends.
struct Agent(Child);

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn the_agent_follows_the_claude_logs_of_its_home() {
    let dir = std::env::temp_dir().join(format!("agent-run-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let project = dir.join("home/.claude/projects/p");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("s-1.jsonl"),
        concat!(
            r#"{"type":"user","timestamp":"2026-11-11T10:15:30.123Z","cwd":"/w","#,
            r#""sessionId":"s-1","message":{"role":"user","content":"hello"}}"#,
            "\n"
        ),
    )
    .unwrap();

    let agent = Agent(
        Command::new(env!("CARGO_BIN_EXE_construct-agent"))
            .arg("run")
            .env("HOME", dir.join("home"))
            .env("XDG_RUNTIME_DIR", dir.join("run"))
            .env("XDG_DATA_HOME", dir.join("data"))
            .env("TMPDIR", dir.join("run"))
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let socket = dir.join("run/construct/agent.sock");
    let mut turns = None;
    for _ in 0..400 {
        if let Ok(reply) = query_status(&socket) {
            let status: serde_json::Value = serde_json::from_str(&reply).unwrap();
            turns = status["connectors"]["claude-code"]["turns"].as_u64();
            if turns == Some(1) {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(agent);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(turns, Some(1), "the agent did not read the log");
}
