//! The socket status call, and who can reach the socket.
#![cfg(unix)]

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::thread;

use construct_agent::ipc::{Server, query_status};

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
    Server::bind(&path).expect("stale socket is replaced");
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
