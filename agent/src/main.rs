use std::process::ExitCode;

use construct_agent::autostart;

const USAGE: &str = "usage: construct-agent <command>

commands:
  install-service    start the agent at every login and start it now
  uninstall-service  stop the agent and remove it from login
  status             ask the running agent for its status
  run                run the agent in the foreground (the service runs this)";

fn main() -> ExitCode {
    let cmd = std::env::args().nth(1);
    let result = match cmd.as_deref() {
        Some("install-service") => install(),
        Some("uninstall-service") => uninstall(),
        Some("status") => return status(),
        Some("run") => run(),
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn install() -> std::io::Result<()> {
    let exe = std::env::current_exe()?.canonicalize()?;
    if exe.to_str().is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the path of the agent binary is not valid UTF-8; move it and try again",
        ));
    }
    autostart::current()?.install(&exe)?;
    println!("installed and started {}", autostart::SERVICE_NAME);
    Ok(())
}

fn uninstall() -> std::io::Result<()> {
    autostart::current()?.uninstall()?;
    println!("removed {}", autostart::SERVICE_NAME);
    Ok(())
}

#[cfg(unix)]
fn run() -> std::io::Result<()> {
    construct_agent::ipc::serve(&construct_agent::ipc::socket_path()?)
}

#[cfg(not(unix))]
fn run() -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "the agent socket is not implemented on this platform yet",
    ))
}

/// Print the agent's status line and exit 0 when it reports `running`; print
/// `stopped` with the reason and exit 1 otherwise.
#[cfg(unix)]
fn status() -> ExitCode {
    let reply = construct_agent::ipc::socket_path()
        .and_then(|p| construct_agent::ipc::query_status(&p))
        .and_then(|line| running_reply(&line));
    match reply {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("{}", stopped(&e.to_string()));
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(unix))]
fn status() -> ExitCode {
    println!("{}", stopped("unsupported platform"));
    ExitCode::FAILURE
}

/// The reply line itself when it is JSON saying `"status":"running"`.
#[cfg(unix)]
fn running_reply(line: &str) -> std::io::Result<String> {
    let value: serde_json::Value = serde_json::from_str(line).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, format!("bad reply: {e}"))
    })?;
    if value["status"] == "running" {
        Ok(line.to_string())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the agent did not report running",
        ))
    }
}

fn stopped(reason: &str) -> String {
    serde_json::json!({ "status": "stopped", "reason": reason }).to_string()
}
