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
#[allow(clippy::too_many_lines)] // one linear start-up sequence
fn run() -> std::io::Result<()> {
    use construct_agent::database::{self, Database, Health, State};
    use construct_agent::host;
    use tokio::signal::unix::{signal, SignalKind};

    let socket = construct_agent::ipc::socket_path()?;
    let paths = database::Paths::from_env()?;
    let host_paths = host::Paths::from_env()?;
    // A file left by a host that is gone names a dead port and a token nobody serves; it must
    // not stay readable while the database starts, or when it fails to.
    host::remove_connection_file(&host_paths)?;
    let (health, construct_health) = (Health::default(), Health::default());
    let connectors = construct_agent::follow::Stats::default();
    let model_state = construct_agent::model::ModelState::default();
    let settings = open_settings();
    let server = with_settings(
        construct_agent::ipc::Server::bind(&socket)?
            .with_database(health.clone())
            .with_construct(construct_health.clone())
            .with_connectors(connectors.clone())
            .with_model(model_state.clone()),
        settings.as_ref(),
    );
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

    // Kept until the function returns, after the runtime: the follower ends when it is dropped.
    let _follow_alive = spawn_follower(&connectors, settings.as_ref());

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
        // The Construct host needs the database; without one it does not start.
        let started = start_construct_until_stopped(
            database.as_ref(),
            &host_paths,
            &construct_health,
            &model_state,
            &mut stop_signal,
        )
        .await;
        let mut host = match started {
            Ok(host) => host,
            Err(sig) => {
                eprintln!("{sig} while the Construct host was starting; stopped");
                stop_all(None, database.as_ref()).await;
                return Ok(());
            }
        };
        // A database that stops accepting connections is reported as failed and the agent exits
        // with an error, so the service manager starts it again and the database with it. A start that
        // failed stays `failed`: restarting would only repeat it. The same holds for a host that stops.
        let watch = watch_database(database.as_ref());
        let watch_host = watch_construct(host.as_mut());
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
            res = watch_host => {
                construct_health.set(State::Failed);
                Err(res)
            }
        };
        stop_all(host, database.as_ref()).await;
        result
    })
}

/// The settings file. A file that cannot be read must not stop the agent: then `settings-*`
/// answer an error and no log reader starts.
#[cfg(unix)]
fn open_settings() -> Option<construct_agent::settings::Settings> {
    use construct_agent::settings;

    match settings::config_dir().and_then(|dir| settings::Settings::open(&dir)) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("settings unavailable: {e}");
            None
        }
    }
}

/// Serve the settings commands from `settings`; without them they answer an error.
#[cfg(unix)]
fn with_settings(
    server: construct_agent::ipc::Server,
    settings: Option<&construct_agent::settings::Settings>,
) -> construct_agent::ipc::Server {
    match settings {
        Some(s) => server.with_settings(s.clone()),
        None => server,
    }
}

/// Starts the follower of the session files of every known tool, on the agent's data directory.
/// The log readers need no database: they scan on a thread of their own, so neither the first
/// scan nor a slow disk holds up `status`. The bound socket makes this the only agent of the
/// user, so it is the one writer of the records.
#[cfg(unix)]
fn spawn_follower(
    stats: &construct_agent::follow::Stats,
    settings: Option<&construct_agent::settings::Settings>,
) -> std::sync::mpsc::Sender<()> {
    use construct_agent::ipc;

    let tools = connectors::Registry::builtin();
    let Some(settings) = settings else {
        // Without the user's switches nothing is collected: no reader starts.
        for tool in tools.iter() {
            stats.not_started(tool.name(), "not started: settings unavailable");
        }
        return std::sync::mpsc::channel().0;
    };
    let paths = ipc::home_dir().and_then(|home| Ok((home, ipc::data_root()?)));
    let settings = settings.clone();
    let switch: construct_agent::follow::Switch =
        std::sync::Arc::new(move |tool| settings.collects(tool));
    construct_agent::follow::spawn_switched(tools, paths, stats, Some(switch))
}

/// Start the Construct host on `db` ([`start_construct`]) and wait for it, unless `stop_signal`
/// fires first. Returns:
/// - `Ok(Some(host))`: the host is running, and `construct` is `ready`;
/// - `Ok(None)`: there is no host, and `construct` is `failed`. This is the case when there is no
///   database and when the host failed to start; the agent keeps running without it;
/// - `Err(signal)`: the signal arrived first. The host that was begun is stopped, and awaited,
///   before this returns, so the caller may stop the database at once.
#[cfg(unix)]
async fn start_construct_until_stopped<S>(
    db: Option<&construct_agent::database::Database>,
    paths: &construct_agent::host::Paths,
    construct_health: &construct_agent::database::Health,
    model_state: &construct_agent::model::ModelState,
    stop_signal: &mut S,
) -> Result<Option<construct_agent::host::Host>, &'static str>
where
    S: std::future::Future<Output = &'static str> + Unpin,
{
    use construct_agent::database::State;

    let Some(db) = db else {
        construct_health.set(State::Failed);
        return Ok(None);
    };
    let abort = tokio_util::sync::CancellationToken::new();
    let start = start_construct(db, paths, construct_health, model_state, &abort);
    tokio::pin!(start);
    tokio::select! {
        host = &mut start => Ok(host),
        sig = stop_signal => {
            abort.cancel();
            stop_all(start.await, None).await;
            Err(sig)
        }
    }
}

/// Create the graph storage database and start the Construct host on it, and record the
/// outcome in `health`; a host that fails to start leaves the agent running without it.
#[cfg(unix)]
async fn start_construct(
    db: &construct_agent::database::Database,
    paths: &construct_agent::host::Paths,
    health: &construct_agent::database::Health,
    model_state: &construct_agent::model::ModelState,
    abort: &tokio_util::sync::CancellationToken,
) -> Option<construct_agent::host::Host> {
    use construct_agent::database::State;
    use construct_agent::host::{Host, GRAPH_DBNAME};

    let model = load_model();
    let started = async {
        db.ensure_database(GRAPH_DBNAME).await?;
        Host::start(
            paths,
            db.paths(),
            model.as_ref().map(|m| (m, model_state)),
            abort,
        )
        .await
    };
    match started.await {
        Ok(host) => {
            health.set(State::Ready);
            eprintln!("construct ready on 127.0.0.1:{}", host.connection().port);
            Some(host)
        }
        Err(e) => {
            health.set(State::Failed);
            eprintln!("construct failed: {e:#}");
            None
        }
    }
}

/// The model of `agent.toml`. A missing file, or one that is refused, leaves the host without a
/// model: it takes records and drops them. The reason is printed; the key never is.
#[cfg(unix)]
fn load_model() -> Option<construct_agent::model::ModelConfig> {
    use construct_agent::model::ModelConfig;
    match ModelConfig::load_default() {
        Ok(Some(model)) => {
            eprintln!("model: {} at {}", model.model, model.base_url);
            Some(model)
        }
        Ok(None) => {
            eprintln!("model: not configured, records are not processed");
            None
        }
        Err(e) => {
            eprintln!("model: agent.toml not used: {e}");
            None
        }
    }
}

/// Resolves when PostgreSQL stops accepting connections; never without a database.
#[cfg(unix)]
async fn watch_database(database: Option<&construct_agent::database::Database>) {
    match database {
        Some(db) => db.watch().await,
        None => std::future::pending::<()>().await,
    }
}

/// Resolves with the reason when a Construct host that was running stops on its own; never
/// without a host.
#[cfg(unix)]
async fn watch_construct(host: Option<&mut construct_agent::host::Host>) -> std::io::Error {
    match host {
        Some(host) => match host.stopped().await {
            Ok(()) => std::io::Error::other("the Construct host stopped"),
            Err(e) => std::io::Error::other(format!("the Construct host failed: {e:#}")),
        },
        None => std::future::pending().await,
    }
}

/// Stop the Construct host first, since it uses the database, then the database.
#[cfg(unix)]
async fn stop_all(
    host: Option<construct_agent::host::Host>,
    database: Option<&construct_agent::database::Database>,
) {
    if let Some(host) = host {
        if let Err(e) = host.stop().await {
            eprintln!("construct: {e:#}");
        }
    }
    if let Some(db) = database {
        if let Err(e) = db.stop().await {
            eprintln!("{e}");
        }
    }
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
