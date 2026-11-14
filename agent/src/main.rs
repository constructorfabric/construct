use std::process::ExitCode;

use construct_agent::autostart;

const USAGE: &str = "usage: construct-agent <command>

commands:
  install-service    start the agent at every login and start it now
  ensure-service     install-service, unless it is installed for this binary already
  uninstall-service  stop the agent and remove it from login
  status             ask the running agent for its status
  run                run the agent in the foreground (the service runs this)";

fn main() -> ExitCode {
    let cmd = std::env::args().nth(1);
    let result = match cmd.as_deref() {
        Some("install-service") => install(),
        Some("ensure-service") => ensure(),
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

/// The path of this binary, which the service file must be able to hold.
fn agent_exe() -> std::io::Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?.canonicalize()?;
    if exe.to_str().is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the path of the agent binary is not valid UTF-8; move it and try again",
        ));
    }
    Ok(exe)
}

fn install() -> std::io::Result<()> {
    autostart::current()?.install(&agent_exe()?)?;
    println!("installed and started {}", autostart::SERVICE_NAME);
    Ok(())
}

fn ensure() -> std::io::Result<()> {
    autostart::current()?.ensure(&agent_exe()?)?;
    println!("{} is registered", autostart::SERVICE_NAME);
    Ok(())
}

fn uninstall() -> std::io::Result<()> {
    autostart::current()?.uninstall()?;
    println!("removed {}", autostart::SERVICE_NAME);
    Ok(())
}

/// Bind the agent socket first (a second agent must fail here, before it touches the
/// database), serve it on a thread, start the database in the background so `status` never
/// waits for it, and stop the database on SIGTERM or SIGINT.
#[cfg(unix)]
fn run() -> std::io::Result<()> {
    use construct_agent::database::{self, Database, Health, State};
    use tokio::signal::unix::{signal, SignalKind};

    let socket = construct_agent::ipc::socket_path()?;
    let paths = database::Paths::from_env()?;
    let health = Health::default();

    let server = construct_agent::ipc::Server::bind(&socket)?.with_database(health.clone());
    eprintln!(
        "construct-agent {} listening on {}",
        env!("CARGO_PKG_VERSION"),
        server.path().display()
    );
    let (served_tx, served_rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        server.serve();
        let _ = served_tx.send(());
    });

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let (mut term, mut int) = rt.block_on(async {
        Ok::<_, std::io::Error>((
            signal(SignalKind::terminate())?,
            signal(SignalKind::interrupt())?,
        ))
    })?;

    rt.block_on(async {
        let stop_signal = async {
            tokio::select! {
                _ = term.recv() => "SIGTERM",
                _ = int.recv() => "SIGINT",
            }
        };
        tokio::pin!(stop_signal);
        // The start future lives only inside this block, so a signal drops it before
        // the leftover PostgreSQL is stopped.
        let started = {
            let start = Database::start(paths.clone());
            tokio::select! {
                res = start => Ok(res),
                sig = &mut stop_signal => Err(sig),
            }
        };
        let database = match started {
            Ok(Ok(db)) => {
                health.set(State::Ready);
                eprintln!("database ready on {}", db.paths().socket_dir.display());
                Some(db)
            }
            Ok(Err(e)) => {
                health.set(State::Failed);
                eprintln!("database failed: {e}");
                None
            }
            Err(sig) => {
                eprintln!("{sig} while the database was starting; stopping");
                database::stop_leftover(&paths).await;
                return Ok(());
            }
        };
        // A database that stops accepting connections is reported as failed and the agent exits
        // with an error, so the service manager starts it again and the database with it. A start that
        // failed stays `failed`: restarting would only repeat it.
        let watch = async {
            match &database {
                Some(db) => db.watch().await,
                None => std::future::pending::<()>().await,
            }
        };
        let result = tokio::select! {
            sig = &mut stop_signal => {
                eprintln!("{sig}: stopping");
                Ok(())
            }
            _ = served_rx => Err(std::io::Error::other("the socket server stopped")),
            () = watch => {
                health.set(State::Failed);
                Err(std::io::Error::other("PostgreSQL stopped accepting connections"))
            }
        };
        if let Some(db) = database {
            if let Err(e) = db.stop().await {
                eprintln!("{e}");
            }
        }
        result
    })
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
