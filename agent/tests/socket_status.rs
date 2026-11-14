//! The socket status call, and who can reach the socket.
#![cfg(unix)]

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::thread;

use construct_agent::ipc::{query_status, Server};

fn temp_socket(name: &str) -> std::path::PathBuf {
    std::env::temp_dir()
        .join(format!("construct-agent-{name}-{}", std::process::id()))
        .join("construct")
        .join("agent.sock")
}

#[test]
fn status_call_answers_running_and_socket_is_user_only() {
    let path = temp_socket("status");
    let server = Server::bind(&path).unwrap();
    thread::spawn(move || server.serve());

    let reply = query_status(&path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(v["status"], "running");
    assert_eq!(v["pid"], std::process::id());

    let sock_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    let dir_mode = std::fs::metadata(path.parent().unwrap())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(sock_mode, 0o600);
    assert_eq!(dir_mode, 0o700);
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

#[test]
fn unknown_command_gets_an_error_line() {
    let path = temp_socket("unknown");
    let server = Server::bind(&path).unwrap();
    thread::spawn(move || server.serve());

    let mut s = UnixStream::connect(&path).unwrap();
    s.write_all(b"explode\n").unwrap();
    let mut reply = String::new();
    s.read_to_string(&mut reply).unwrap();
    assert!(reply.contains("unknown command"), "{reply}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

#[test]
fn second_agent_refuses_a_live_socket_but_replaces_a_stale_one() {
    let path = temp_socket("stale");
    let first = Server::bind(&path).unwrap();
    let err = Server::bind(&path)
        .err()
        .expect("live socket must be refused");
    assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);

    drop(first); // listener closed, socket file stays: a stale file
    assert!(path.exists());
    // Another test in this process may be spawning a child at this moment, and the child holds a
    // copy of the closed listener until it execs: the socket still answers for a few milliseconds.
    let mut bound = Server::bind(&path);
    for _ in 0..100 {
        if bound.is_ok() {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(20));
        bound = Server::bind(&path);
    }
    bound.expect("stale socket is replaced");
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

#[test]
fn a_peer_that_closes_without_replying_is_an_error_not_running() {
    let path = temp_socket("silent");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    thread::spawn(move || {
        // Read the request, then close without answering.
        let (stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        std::io::BufRead::read_line(&mut std::io::BufReader::new(&stream), &mut line).unwrap();
    });
    let err = query_status(&path).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

#[test]
fn status_command_exits_1_with_stopped_when_no_agent_runs() {
    let dir = std::env::temp_dir().join(format!("construct-agent-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_construct-agent"))
        .arg("status")
        .env("XDG_RUNTIME_DIR", &dir)
        .env("TMPDIR", &dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["status"], "stopped");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn status_command_exits_0_when_the_agent_runs() {
    let dir = std::env::temp_dir().join(format!("construct-agent-cli2-{}", std::process::id()));
    let path = dir.join("construct").join("agent.sock");
    let server = Server::bind(&path).unwrap();
    thread::spawn(move || server.serve());
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_construct-agent"))
        .arg("status")
        .env("XDG_RUNTIME_DIR", &dir)
        .env("TMPDIR", &dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["status"], "running");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_file_that_is_not_a_socket_is_refused_and_left_alone() {
    let path = temp_socket("notsock");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "precious").unwrap();
    let err = Server::bind(&path)
        .err()
        .expect("a regular file must be refused");
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "precious");
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

#[test]
fn a_socket_path_without_a_parent_directory_is_refused() {
    let err = Server::bind(std::path::Path::new("/")).err().unwrap();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn status_reports_the_database_and_construct_states_each_on_its_own() {
    use construct_agent::database::{Health, State};

    let path = temp_socket("database");
    let database = Health::default();
    let construct = Health::default();
    let server = Server::bind(&path)
        .unwrap()
        .with_database(database.clone())
        .with_construct(construct.clone());
    thread::spawn(move || server.serve());

    let states = |path: &std::path::Path| {
        let v: serde_json::Value = serde_json::from_str(&query_status(path).unwrap()).unwrap();
        (
            v["database"].as_str().unwrap().to_string(),
            v["construct"].as_str().unwrap().to_string(),
        )
    };
    let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
    assert_eq!(states(&path), pair("starting", "starting"));
    database.set(State::Ready);
    assert_eq!(states(&path), pair("ready", "starting"));
    construct.set(State::Ready);
    assert_eq!(states(&path), pair("ready", "ready"));
    construct.set(State::Failed);
    assert_eq!(states(&path), pair("ready", "failed"));
    database.set(State::Failed);
    construct.set(State::Ready);
    assert_eq!(states(&path), pair("failed", "ready"));
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

const KEY: &str = "sk-test-key-0123456789";

#[allow(clippy::unwrap_used)] // test helper, outside a `#[test]` fn
fn settings_server(name: &str) -> (std::path::PathBuf, construct_agent::settings::Settings) {
    let path = temp_socket(name);
    let settings = construct_agent::settings::Settings::open(
        &path.parent().unwrap().parent().unwrap().join("config"),
    )
    .unwrap();
    let server = Server::bind(&path).unwrap().with_settings(settings.clone());
    thread::spawn(move || server.serve());
    (path, settings)
}

/// Send `request` as one line and return the reply text.
#[allow(clippy::unwrap_used)] // test helper, outside a `#[test]` fn
fn ask(path: &std::path::Path, request: &[u8]) -> String {
    let mut s = UnixStream::connect(path).unwrap();
    s.write_all(request).unwrap();
    s.write_all(b"\n").unwrap();
    let mut reply = String::new();
    s.read_to_string(&mut reply).unwrap();
    reply
}

#[test]
fn settings_set_and_get_over_the_socket_never_answer_the_key() {
    let (path, settings) = settings_server("settings");
    let before: serde_json::Value = serde_json::from_str(&ask(&path, b"settings-get")).unwrap();
    assert_eq!(before["model"]["api_key_set"], false);
    assert_eq!(before["collection"]["enabled"], true);
    let status: serde_json::Value = serde_json::from_str(&ask(&path, b"status")).unwrap();
    assert_eq!(status["model"], "not set");
    assert_eq!(status["model_state"], "not_configured");

    let set = format!(
        r#"settings-set {{"model":{{"base_url":"https://api.example.com/v1","model":"m1","api_key":"{KEY}"}},"collection":{{"codex":false}}}}"#
    );
    let set_reply = ask(&path, set.as_bytes());
    let get_reply = ask(&path, b"settings-get");
    let status_reply = ask(&path, b"status");
    for reply in [&set_reply, &get_reply, &status_reply] {
        assert!(!reply.contains(KEY), "{reply}");
        assert!(
            !reply.contains("api_key\":\"") && !reply.contains("sk-"),
            "{reply}"
        );
    }
    let v: serde_json::Value = serde_json::from_str(&get_reply).unwrap();
    assert_eq!(v["model"]["model"], "m1");
    assert_eq!(v["model"]["api_key_set"], true);
    assert_eq!(v["collection"]["codex"], false);
    let status: serde_json::Value = serde_json::from_str(&status_reply).unwrap();
    assert_eq!(status["model"], "m1");
    assert_eq!(status["model_state"], "configured");
    // The readers inside the agent see the change at once.
    assert_eq!(settings.model().api_key.as_deref(), Some(KEY));
    assert!(!settings.collection().codex);

    let remove = ask(&path, br#"settings-set {"model":{"remove_key":true}}"#);
    assert!(remove.contains("\"api_key_set\":false"), "{remove}");
    assert_eq!(settings.model().api_key, None);
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

#[test]
fn a_refused_settings_set_does_not_repeat_the_request() {
    let (path, settings) = settings_server("refused");
    for request in [
        format!(r#"settings-set {{"model":{{"api_key":"{KEY}","extra":1}}}}"#),
        format!(r#"settings-set {{"model":{{"api_key":"{KEY}"#),
        format!(
            r#"settings-set {{"model":{{"base_url":"http://example.com","api_key":"{KEY}"}}}}"#
        ),
        format!("settings-set{KEY}"),
        format!("settings-sett {KEY}"),
        format!("{KEY} settings-set"),
        format!("status {KEY}"),
    ] {
        let reply = ask(&path, request.as_bytes());
        assert!(reply.contains("\"error\""), "{reply}");
        assert!(!reply.contains("sk-"), "{reply}");
        assert!(!reply.contains(KEY), "{reply}");
    }
    assert_eq!(settings.model().api_key, None);
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

#[test]
fn a_request_line_over_the_cap_is_refused() {
    let (path, settings) = settings_server("long");
    let padding = "a".repeat(construct_agent::ipc::MAX_REQUEST);
    let request = format!(r#"settings-set {{"model":{{"model":"{padding}","api_key":"{KEY}"}}}}"#);
    let reply = ask(&path, request.as_bytes());
    assert!(reply.contains("request too long"), "{reply}");
    assert!(!reply.contains(KEY));
    assert_eq!(settings.model().model, "");

    // A line just under the cap is served.
    let padding = "a".repeat(construct_agent::ipc::MAX_REQUEST - 64);
    let request = format!(r#"settings-set {{"model":{{"model":"{padding}"}}}}"#);
    let reply = ask(&path, request.as_bytes());
    assert!(
        reply.contains("api_key_set"),
        "{}",
        &reply[..80.min(reply.len())]
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

#[test]
fn settings_commands_without_settings_answer_an_error() {
    let path = temp_socket("nosettings");
    let server = Server::bind(&path).unwrap();
    thread::spawn(move || server.serve());
    let reply = ask(&path, b"settings-get");
    assert!(reply.contains("not available"), "{reply}");
    let status: serde_json::Value = serde_json::from_str(&ask(&path, b"status")).unwrap();
    assert_eq!(status["model"], "not set");
    assert_eq!(status["model_state"], "not_configured");
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

#[test]
fn status_shows_the_saved_model_name_and_whether_the_settings_hold_a_complete_model() {
    use construct_agent::model::ModelState;
    use construct_agent::settings::Settings;

    let path = temp_socket("model");
    let root = path.parent().unwrap().parent().unwrap().to_path_buf();
    let settings = Settings::open(&root.join("config")).unwrap();
    let model = ModelState::default();
    let server = Server::bind(&path)
        .unwrap()
        .with_settings(settings.clone())
        .with_model(model.clone());
    thread::spawn(move || server.serve());

    let status = |path: &std::path::Path| -> serde_json::Value {
        serde_json::from_str(&query_status(path).unwrap()).unwrap()
    };
    let v = status(&path);
    assert_eq!(
        (v["model"].as_str(), v["model_state"].as_str()),
        (Some("not set"), Some("not_configured"))
    );
    // A name alone is not a model: the address and the key are missing.
    ask(&path, br#"settings-set {"model":{"model":"m1"}}"#);
    assert_eq!(status(&path)["model_state"], "not_configured");
    let set = format!(
        r#"settings-set {{"model":{{"base_url":"https://api.example.com/v1","api_key":"{KEY}"}}}}"#
    );
    ask(&path, set.as_bytes());
    let v = status(&path);
    assert_eq!(
        (v["model"].as_str(), v["model_state"].as_str()),
        (Some("m1"), Some("configured"))
    );
    assert!(!v.to_string().contains(KEY));
    ask(&path, br#"settings-set {"model":{"remove_key":true}}"#);
    assert_eq!(status(&path)["model_state"], "not_configured");
    let _ = std::fs::remove_dir_all(root);
}
