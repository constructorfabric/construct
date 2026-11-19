//! App <-> agent channel: one request per connection over a Unix socket in the
//! user's runtime directory. The directory and the socket are user-only (`DIR_MODE`,
//! `SOCKET_MODE`), so only the user can connect. The agent opens no TCP port.
//!
//! Protocol: the client sends one line, the agent answers one JSON line. Commands:
//! `status` (the reply has `service`: `registered` or `not_registered`, whether the login
//! service of the agent is registered, and no `service` when that cannot be told; `database`
//! and `construct` are each `{"state": ..., "reason": ...}`, see [`status_response`]),
//! `settings-get`, `settings-set <JSON object>` (the fields to change; see `settings`), and
//! `settings-test [JSON object]` (the checks on the model; see `model_test`).
//! `backfill [JSON object]` sends the prompts of the past to Construct (see `manage`).
//! `memory-erase` switches all collection off, has Construct erase everything about the person and
//! erases the agent's own copies: the collected sessions, the chats and the activity time (see
//! `erase`).
//! `dashboard-get` answers the current dashboard document and, when the file does not pass its
//! type, the error with its place (see `dashboard_document`). `dashboard-hide <JSON object>`,
//! `dashboard-show <JSON object>` (`{"project": <folder>}`) and `dashboard-rename <JSON object>`
//! (`{"project": <folder>, "name": <name to show>}`) change the document in the file and answer
//! as `dashboard-get` does once the change is read back; an error is a plain text.
//! `dashboard-numbers <JSON object>` answers the numbers of a widget, or of a period, for the days
//! the request lists (see `dashboard_numbers`); the answer is longer than a status.
//! `dashboard-sessions <JSON object>`, `dashboard-summary <JSON object>` and `dashboard-state` answer
//! the sessions behind a group, the summary of a day and how far the data is (see
//! `dashboard_details`).
//! `dashboard-setup-start [JSON object]` (`{"days": <period>}`, or nothing) starts the setup of the
//! dashboard as a background job of the agent, `dashboard-setup-status` answers where the run is
//! and how much it has read and sent, and `dashboard-setup-cancel` stops it (see
//! `dashboard_setup`).
//! `settings-test` answers with several JSON lines when its request has `"progress": true`.
//! The chat commands (see `chat`) answer with several JSON lines: `chat-send <JSON object>`
//! streams an answer, `chat-get` sends one line per record. A request line is at most
//! `MAX_REQUEST` bytes, and a `chat-send` line at most `chat::MAX_REQUEST`.

use std::io;
use std::path::PathBuf;

/// Socket file name inside the `construct` runtime directory.
pub const SOCKET_NAME: &str = "agent.sock";

/// The JSON line the agent answers to `status`.
///
/// `database` and `construct` are objects: `state` is `starting`, `ready` or `failed`, and
/// `reason`, only when the part is not ready, is a fixed short text of the agent (see
/// `database::reason`), never a path, a secret or the text of an error. The app accepts a
/// part as that object or as the state alone, a plain string.
#[must_use]
// One argument per field of the reply; a struct would only move the list.
#[allow(clippy::too_many_arguments)]
pub fn status_response(
    pid: u32,
    database: &serde_json::Value,
    construct: &serde_json::Value,
    connectors: &serde_json::Value,
    model: &str,
    model_state: &str,
    service_registered: Option<bool>,
    facts: Option<serde_json::Value>,
    activity: Option<serde_json::Value>,
    screen: Option<serde_json::Value>,
) -> String {
    let mut reply = serde_json::json!({
        "status": "running",
        "version": env!("CARGO_PKG_VERSION"),
        "pid": pid,
        "database": database,
        "construct": construct,
        "connectors": connectors,
        "model": model,
        "model_state": model_state,
    });
    if let Some(facts) = facts {
        reply["facts"] = facts;
    }
    if let Some(activity) = activity {
        reply["activity"] = activity;
    }
    if let Some(screen) = screen {
        reply["screen"] = screen;
    }
    if let Some(registered) = service_registered {
        reply["service"] = if registered {
            "registered"
        } else {
            "not_registered"
        }
        .into();
    }
    reply.to_string()
}

/// Longest request line the agent reads, in bytes, for every command but `chat-send`. A longer
/// one is refused.
pub const MAX_REQUEST: usize = 16 * 1024;
/// Longest command word the agent repeats in an error.
const MAX_ECHO: usize = 24;

/// The JSON line the agent answers to anything else.
///
/// The line may be a `settings-set` with a key in it, so no argument is ever repeated: the
/// command is named only when the whole line is one short word of lowercase letters, digits
/// and dashes.
#[must_use]
pub fn unknown_command_response(line: &str) -> String {
    let line = line.trim();
    let plain = !line.is_empty()
        && line.len() <= MAX_ECHO
        && line
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if plain {
        serde_json::json!({ "error": "unknown command", "command": line }).to_string()
    } else {
        serde_json::json!({ "error": "unknown command" }).to_string()
    }
}

/// The JSON line for a refused request; `message` is a fixed text.
#[must_use]
pub fn error_response(message: &str) -> String {
    serde_json::json!({ "error": message }).to_string()
}

/// The path in the environment variable `var`, when it is set to an absolute path.
#[must_use]
pub fn absolute_env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// The `construct` directory inside the runtime directory (`$XDG_RUNTIME_DIR` on
/// Linux, the per-user `$TMPDIR` on macOS).
///
/// # Errors
/// Fails when that variable is not set to an absolute path.
pub fn runtime_dir() -> io::Result<PathBuf> {
    let var = if cfg!(target_os = "macos") {
        "TMPDIR"
    } else {
        "XDG_RUNTIME_DIR"
    };
    let dir = absolute_env_path(var)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{var} is not set")))?;
    Ok(dir.join("construct"))
}

/// Socket path of the agent.
///
/// # Errors
/// Fails when the runtime directory is not set, see [`runtime_dir`].
pub fn socket_path() -> io::Result<PathBuf> {
    Ok(runtime_dir()?.join(SOCKET_NAME))
}

#[cfg(unix)]
pub use unix::{Server, create_private_dir, query_status, request};

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::io::{self, BufRead, BufReader, Write};
    use std::os::unix::fs::{DirBuilderExt, FileTypeExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, mpsc};
    use std::thread;
    use std::time::Duration;

    use super::{MAX_REQUEST, error_response, status_response, unknown_command_response};
    use crate::activity::Feed;
    use crate::chat::{self, Chat};
    use crate::dashboard_details::Details;
    use crate::dashboard_document::Dashboard;
    use crate::dashboard_numbers::Numbers;
    use crate::dashboard_projects::Edit;
    use crate::dashboard_setup::SetupJob;
    use crate::database::Health;
    use crate::follow::Stats;
    use crate::manage::Manager;
    use crate::model::{ModelHealth, ModelState};
    use crate::screen::Screen;
    use crate::settings::{Settings, Update};

    /// Mode of the `construct` runtime directory: the user only.
    const DIR_MODE: u32 = 0o700;
    /// Mode of the socket file: the user only.
    const SOCKET_MODE: u32 = 0o600;
    /// How much of an over-long request line is read and dropped after refusing it.
    const DRAIN_LIMIT: usize = 1024 * 1024;
    /// Pause after a failed `accept`, so a persistent error (no free file
    /// descriptors) does not spin the CPU.
    const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);
    /// How long one `status` call may take in total, connect included.
    const QUERY_TIMEOUT: Duration = Duration::from_secs(5);

    /// Create `dir` (and its parents) and make it user-only.
    ///
    /// # Errors
    /// Fails when the directory cannot be created or its mode cannot be set.
    pub fn create_private_dir(dir: &Path) -> io::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(DIR_MODE)
            .create(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(DIR_MODE))
    }

    pub struct Server {
        listener: UnixListener,
        path: PathBuf,
        database: Health,
        construct: Health,
        connectors: Stats,
        model: ModelState,
        service: Arc<dyn Fn() -> Option<bool> + Send + Sync>,
        settings: Option<Settings>,
        chat: Option<Chat>,
        manage: Option<Manager>,
        activity: Option<Feed>,
        dashboard: Option<Dashboard>,
        numbers: Option<Numbers>,
        details: Option<Details>,
        setup: Option<SetupJob>,
        screen: Option<Screen>,
    }

    impl Server {
        /// Bind the socket, replacing a stale file left by a dead agent but
        /// refusing to replace a live one.
        ///
        /// # Errors
        /// Fails when the socket path has no parent, another agent is listening
        /// on it, or the directory or socket cannot be created.
        pub fn bind(path: &Path) -> io::Result<Self> {
            let Some(dir) = path.parent() else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "socket path has no parent directory",
                ));
            };
            create_private_dir(dir)?;
            remove_stale_socket(path)?;
            let listener = UnixListener::bind(path)?;
            if let Err(e) = fs::set_permissions(path, fs::Permissions::from_mode(SOCKET_MODE)) {
                let _ = fs::remove_file(path);
                return Err(e);
            }
            Ok(Self {
                listener,
                path: path.to_path_buf(),
                database: Health::default(),
                construct: Health::default(),
                connectors: Stats::default(),
                model: ModelState::default(),
                service: Arc::new(|| None),
                settings: None,
                chat: None,
                manage: None,
                activity: None,
                dashboard: None,
                numbers: None,
                details: None,
                setup: None,
                screen: None,
            })
        }

        /// Report this database state in `status`.
        #[must_use]
        pub fn with_database(mut self, database: Health) -> Self {
            self.database = database;
            self
        }

        /// Report this Construct host state in `status`.
        #[must_use]
        pub fn with_construct(mut self, construct: Health) -> Self {
            self.construct = construct;
            self
        }

        /// Report these log readers in `status`.
        #[must_use]
        pub fn with_connectors(mut self, connectors: Stats) -> Self {
            self.connectors = connectors;
            self
        }

        /// Report the outcome of the model calls in `status` as `model_state`: `not_configured`, `configured` or `key_rejected`.
        #[must_use]
        pub fn with_model(mut self, model: ModelState) -> Self {
            self.model = model;
            self
        }

        /// Report in `status` as `service` whether `check` finds the login service registered; a
        /// `None` leaves the field out. It runs at every `status`, so it must be cheap: a file
        /// test, no program started.
        #[must_use]
        pub fn with_service(
            mut self,
            check: impl Fn() -> Option<bool> + Send + Sync + 'static,
        ) -> Self {
            self.service = Arc::new(check);
            self
        }

        /// Serve `settings-get`, `settings-set` and `settings-test` from these settings, and
        /// report their
        /// model in `status`. Without them the settings commands answer an error.
        #[must_use]
        pub fn with_settings(mut self, settings: Settings) -> Self {
            self.settings = Some(settings);
            self
        }

        /// Serve the chat commands from this chat. Without it they answer an error.
        #[must_use]
        pub fn with_chat(mut self, chat: Chat) -> Self {
            self.chat = Some(chat);
            self
        }

        /// Serve `backfill` from this manager and report it in `status` as `facts`.
        #[must_use]
        pub fn with_manage(mut self, manage: Manager) -> Self {
            self.manage = Some(manage);
            self
        }

        /// Report the activity store in `status` as `activity`.
        #[must_use]
        pub fn with_activity(mut self, activity: Feed) -> Self {
            self.activity = Some(activity);
            self
        }

        /// Serve `dashboard-get` from this document. Without it the command answers an error.
        #[must_use]
        pub fn with_dashboard(mut self, dashboard: Dashboard) -> Self {
            self.dashboard = Some(dashboard);
            self
        }

        /// Serve `dashboard-numbers` from the activity time and the dashboard document. Without
        /// it the command answers an error.
        #[must_use]
        pub fn with_numbers(mut self, numbers: Numbers) -> Self {
            self.numbers = Some(numbers);
            self
        }

        /// Serve `dashboard-sessions`, `dashboard-summary` and `dashboard-state`. Without it
        /// they answer an error.
        #[must_use]
        pub fn with_details(mut self, details: Details) -> Self {
            self.details = Some(details);
            self
        }

        /// Serve `dashboard-setup-start`, `dashboard-setup-status` and `dashboard-setup-cancel`
        /// from this job. Without it they answer an error.
        #[must_use]
        pub fn with_setup(mut self, setup: SetupJob) -> Self {
            self.setup = Some(setup);
            self
        }

        /// Show the screen samples as the `screen` row of `status`.
        #[must_use]
        pub fn with_screen(mut self, screen: Screen) -> Self {
            self.screen = Some(screen);
            self
        }

        #[must_use]
        pub fn path(&self) -> &Path {
            &self.path
        }

        /// Accept connections forever, one thread each.
        ///
        /// # Errors
        /// Never fails today; the signature leaves room for a fatal accept error.
        pub fn serve(&self) -> io::Result<()> {
            for conn in self.listener.incoming() {
                match conn {
                    Ok(stream) => {
                        let database = self.database.clone();
                        let construct = self.construct.clone();
                        let connectors = self.connectors.clone();
                        let model = self.model.clone();
                        let service = Arc::clone(&self.service);
                        let settings = self.settings.clone();
                        let chat = self.chat.clone();
                        let manage = self.manage.clone();
                        let activity = self.activity.clone();
                        let dashboard = self.dashboard.clone();
                        let numbers = self.numbers.clone();
                        let details = self.details.clone();
                        let setup = self.setup.clone();
                        let screen = self.screen.clone();
                        thread::spawn(move || {
                            let shared = Shared {
                                database: &database,
                                construct: &construct,
                                connectors: &connectors,
                                model: &model,
                                service: service.as_ref(),
                                settings: settings.as_ref(),
                                chat: chat.as_ref(),
                                manage: manage.as_ref(),
                                activity: activity.as_ref(),
                                dashboard: dashboard.as_ref(),
                                numbers: numbers.as_ref(),
                                details: details.as_ref(),
                                setup: setup.as_ref(),
                                screen: screen.as_ref(),
                            };
                            if let Err(e) = handle(stream, &shared) {
                                eprintln!("connection error: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("accept error: {e}");
                        thread::sleep(ACCEPT_BACKOFF);
                    }
                }
            }
            Ok(())
        }
    }

    /// Remove a socket file left by a dead agent. A live agent is refused, and
    /// so is anything that is not a socket: the path is never unlinked blindly.
    fn remove_stale_socket(path: &Path) -> io::Result<()> {
        let meta = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        if !meta.file_type().is_socket() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} exists and is not a socket", path.display()),
            ));
        }
        match UnixStream::connect(path) {
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!("another agent is already listening on {}", path.display()),
            )),
            Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => fs::remove_file(path),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// The trimmed line, its command word and the rest of it.
    fn split_command(line: &str) -> (&str, &str, &str) {
        let line = line.trim();
        match line.split_once(char::is_whitespace) {
            Some((command, rest)) => (line, command, rest.trim()),
            None => (line, line, ""),
        }
    }

    /// What `read_request` found.
    enum Request {
        Line(Vec<u8>),
        /// A line over its limit; `chat` when it is a `chat-send`.
        TooLong {
            chat: bool,
        },
    }

    /// Whether `line` starts with the command word `chat-send`.
    fn is_chat_send(line: &[u8]) -> bool {
        line.strip_prefix(b"chat-send")
            .is_some_and(|rest| rest.first().is_none_or(u8::is_ascii_whitespace))
    }

    /// Read one request line without its line end: at most `MAX_REQUEST` bytes, or
    /// `chat::MAX_REQUEST` for a `chat-send`.
    fn read_request(stream: &UnixStream) -> io::Result<Request> {
        let mut line = Vec::new();
        let limit = u64::try_from(MAX_REQUEST + 1).unwrap_or(u64::MAX);
        BufReader::new(io::Read::take(stream, limit)).read_until(b'\n', &mut line)?;
        if line.len() <= MAX_REQUEST {
            return Ok(Request::Line(line));
        }
        let chat = is_chat_send(&line);
        if chat && line.ends_with(b"\n") {
            return Ok(Request::Line(line));
        }
        if chat {
            // The first read took everything it was given, so the second starts where it ended.
            let more = u64::try_from(chat::MAX_REQUEST - MAX_REQUEST).unwrap_or(u64::MAX);
            BufReader::new(io::Read::take(stream, more)).read_until(b'\n', &mut line)?;
            if line.len() <= chat::MAX_REQUEST {
                return Ok(Request::Line(line));
            }
        }
        if !line.ends_with(b"\n") {
            discard_rest(stream);
        }
        Ok(Request::TooLong { chat })
    }

    /// Read and drop the rest of an over-long line, up to `DRAIN_LIMIT` bytes, so that closing
    /// the connection does not reset it before the client has read the refusal.
    fn discard_rest(stream: &UnixStream) {
        let limit = u64::try_from(DRAIN_LIMIT).unwrap_or(u64::MAX);
        let mut reader = BufReader::new(io::Read::take(stream, limit));
        loop {
            let Ok(buf) = reader.fill_buf() else { return };
            if buf.is_empty() {
                return;
            }
            let newline = buf.contains(&b'\n');
            let n = buf.len();
            reader.consume(n);
            if newline {
                return;
            }
        }
    }

    /// What the connection threads answer from.
    struct Shared<'a> {
        database: &'a Health,
        construct: &'a Health,
        connectors: &'a Stats,
        model: &'a ModelState,
        service: &'a (dyn Fn() -> Option<bool> + Send + Sync),
        settings: Option<&'a Settings>,
        chat: Option<&'a Chat>,
        manage: Option<&'a Manager>,
        activity: Option<&'a Feed>,
        dashboard: Option<&'a Dashboard>,
        numbers: Option<&'a Numbers>,
        details: Option<&'a Details>,
        setup: Option<&'a SetupJob>,
        screen: Option<&'a Screen>,
    }

    /// Never log, or return in an error, any part of `line`: it can hold a key.
    fn reply(line: &[u8], shared: &Shared) -> String {
        let settings = shared.settings;
        let Ok(line) = std::str::from_utf8(line) else {
            return error_response("invalid request");
        };
        let (line, command, argument) = split_command(line);
        let (model_name, model_state) = settings.map_or_else(
            || ("not set".to_string(), ModelHealth::NotConfigured),
            |s| {
                let model = s.model();
                let health = shared.model.health(&model);
                let name = if health == ModelHealth::NotConfigured {
                    "not set"
                } else {
                    &model.model
                };
                (name.to_string(), health)
            },
        );
        match (command, settings) {
            ("status", _) if argument.is_empty() => status_response(
                std::process::id(),
                &shared.database.to_json(),
                &shared.construct.to_json(),
                &shared.connectors.to_json(),
                &model_name,
                model_state.as_str(),
                (shared.service)(),
                shared.manage.map(Manager::status),
                shared.activity.map(Feed::to_json),
                shared.screen.map(Screen::status),
            ),
            ("backfill", _) => shared.manage.map_or_else(
                || error_response("backfill is not available"),
                |manage| manage.backfill(argument),
            ),
            ("dashboard-get", _) if argument.is_empty() => shared.dashboard.map_or_else(
                || error_response("the dashboard is not available"),
                Dashboard::to_json_line,
            ),
            ("dashboard-hide" | "dashboard-show" | "dashboard-rename", _) => {
                shared.dashboard.map_or_else(
                    || error_response("the dashboard is not available"),
                    |dashboard| match Edit::parse(command, argument)
                        .map_err(str::to_owned)
                        .and_then(|edit| dashboard.edit(&edit))
                    {
                        Ok(()) => dashboard.to_json_line(),
                        Err(message) => error_response(&message),
                    },
                )
            }
            ("dashboard-numbers", _) => shared.numbers.map_or_else(
                || error_response("the dashboard numbers are not available"),
                |numbers| numbers.answer(argument),
            ),
            ("dashboard-sessions", _) => shared.details.map_or_else(
                || error_response("the dashboard details are not available"),
                |details| details.sessions(argument),
            ),
            ("dashboard-summary", _) => shared.details.map_or_else(
                || error_response("the dashboard details are not available"),
                |details| details.summary(argument),
            ),
            ("dashboard-state", _) if argument.is_empty() => shared.details.map_or_else(
                || error_response("the dashboard details are not available"),
                Details::state,
            ),
            ("dashboard-setup-start", _) => shared.setup.map_or_else(
                || error_response("the dashboard setup is not available"),
                |setup| setup.start(argument),
            ),
            ("dashboard-setup-status", _) if argument.is_empty() => shared.setup.map_or_else(
                || error_response("the dashboard setup is not available"),
                SetupJob::status_line,
            ),
            ("dashboard-setup-cancel", _) if argument.is_empty() => shared.setup.map_or_else(
                || error_response("the dashboard setup is not available"),
                SetupJob::cancel,
            ),
            ("settings-get", Some(s)) if argument.is_empty() => s.get_response(),
            ("settings-set", Some(s)) => {
                let update = Update::parse(argument).map_err(|e| e.as_str().to_string());
                let applied = update.and_then(|update| match shared.manage {
                    Some(manage) => manage.apply_settings(update),
                    None => s.apply(update).map_err(|e| e.as_str().to_string()),
                });
                match applied {
                    Ok(()) => s.get_response(),
                    Err(reason) => error_response(&reason),
                }
            }
            ("memory-erase", _) if argument.is_empty() => shared.manage.map_or_else(
                || error_response("memory-erase is not available"),
                Manager::erase,
            ),
            ("settings-get" | "settings-set", None) => error_response("settings are not available"),
            _ => unknown_command_response(line),
        }
    }

    fn handle(stream: UnixStream, shared: &Shared) -> io::Result<()> {
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        let mut stream = stream;
        let reply = match read_request(&stream)? {
            Request::Line(line) => {
                if let Some(chat_reply) = chat_command(&line, shared, &mut stream)? {
                    return Ok(chat_reply);
                }
                if let Some(test_reply) = settings_test_command(&line, shared, &mut stream)? {
                    return Ok(test_reply);
                }
                reply(&line, shared)
            }
            Request::TooLong { chat: true } => chat::event_error("request too large"),
            Request::TooLong { chat: false } => error_response("request too long"),
        };
        writeln!(stream, "{reply}")
    }

    /// What the multi-line commands give their lines to: writes each line and a newline, and
    /// answers `false` once the client is gone.
    fn line_writer(out: &mut UnixStream) -> impl FnMut(&str) -> bool + '_ {
        |l| out.write_all(format!("{l}\n").as_bytes()).is_ok()
    }

    /// Serve `settings-test`; `None` when the line is not one. With `progress` in its request
    /// it answers with a line for each check, and a Test can take longer than the read timeout
    /// of the request line: a client that is gone is found by a failed write. Never log, or
    /// return in an error, any part of `line`.
    fn settings_test_command(
        line: &[u8],
        shared: &Shared,
        stream: &mut UnixStream,
    ) -> io::Result<Option<()>> {
        let Ok(line) = std::str::from_utf8(line) else {
            return Ok(None);
        };
        let (_, command, argument) = split_command(line);
        if command != "settings-test" {
            return Ok(None);
        }
        let Some(settings) = shared.settings else {
            writeln!(stream, "{}", error_response("settings are not available"))?;
            return Ok(Some(()));
        };
        stream.set_read_timeout(None)?;
        stream.set_write_timeout(Some(chat::WRITE_TIMEOUT))?;
        let mut out = stream.try_clone()?;
        crate::model_test::run(
            argument,
            &settings.model(),
            crate::model_test::DEADLINE,
            &mut line_writer(&mut out),
        );
        Ok(Some(()))
    }

    /// Serve a chat command; `None` when the line is not one. An answer can take minutes, so
    /// the read timeout of the request line is lifted; a client that is gone is found by a
    /// failed write. Never log, or return in an error, any part of `line`.
    fn chat_command(
        line: &[u8],
        shared: &Shared,
        stream: &mut UnixStream,
    ) -> io::Result<Option<()>> {
        let Ok(line) = std::str::from_utf8(line) else {
            return Ok(None);
        };
        let (_, command, argument) = split_command(line);
        if !matches!(
            command,
            "chat-send" | "chat-stop" | "chat-list" | "chat-get" | "chat-delete"
        ) {
            return Ok(None);
        }
        let Some(chat) = shared.chat else {
            let unavailable = if command == "chat-send" {
                chat::event_error("chat is not available")
            } else {
                error_response("chat is not available")
            };
            writeln!(stream, "{unavailable}")?;
            return Ok(Some(()));
        };
        stream.set_read_timeout(None)?;
        stream.set_write_timeout(Some(chat::WRITE_TIMEOUT))?;
        match command {
            "chat-send" => {
                let mut out = stream.try_clone()?;
                chat.send(argument, &mut line_writer(&mut out));
            }
            "chat-stop" => {
                writeln!(
                    stream,
                    "{}",
                    serde_json::json!({ "stopped": chat.stop(argument) })
                )?;
            }
            "chat-list" => writeln!(stream, "{}", chat.list())?,
            "chat-get" => {
                for l in chat.get(argument) {
                    writeln!(stream, "{l}")?;
                }
            }
            _ => writeln!(stream, "{}", chat.delete(argument))?,
        }
        Ok(Some(()))
    }

    /// Ask the agent on `path` for its status; returns the JSON line. The whole
    /// call, connect included, gives up after `QUERY_TIMEOUT`.
    ///
    /// # Errors
    /// Fails when nothing listens on `path`, the agent does not answer in time, or it
    /// sends an empty reply.
    pub fn query_status(path: &Path) -> io::Result<String> {
        let (tx, rx) = mpsc::channel();
        let path = path.to_path_buf();
        thread::spawn(move || {
            let _ = tx.send(ask_status(&path));
        });
        rx.recv_timeout(QUERY_TIMEOUT).unwrap_or_else(|_| {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the agent did not answer in time",
            ))
        })
    }

    /// Send one request line to the agent on `path` and return its one JSON line. The agent
    /// gets `timeout` to answer.
    ///
    /// # Errors
    /// Fails when nothing listens on `path`, the agent does not answer in time, or it
    /// sends an empty reply.
    pub fn request(path: &Path, line: &str, timeout: Duration) -> io::Result<String> {
        let mut stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(QUERY_TIMEOUT))?;
        writeln!(stream, "{line}")?;
        let mut reply = String::new();
        if BufReader::new(stream).read_line(&mut reply)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the agent closed the connection without a reply",
            ));
        }
        Ok(reply.trim().to_string())
    }

    fn ask_status(path: &Path) -> io::Result<String> {
        request(path, "status", QUERY_TIMEOUT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_carries_the_state_and_the_reason_of_the_database_and_construct() {
        let database = serde_json::json!({
            "state": "failed",
            "reason": "pgvector is not built for this platform yet"
        });
        let construct =
            serde_json::json!({ "state": "failed", "reason": "the database is not running" });
        let line = status_response(
            7,
            &database,
            &construct,
            &serde_json::json!({}),
            "not set",
            "not_configured",
            None,
            None,
            None,
            None,
        );
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["status"], "running");
        assert_eq!(v["pid"], 7);
        assert_eq!(v["database"], database);
        assert_eq!(v["construct"], construct);
        assert!(v.get("service").is_none());
        assert!(v.get("facts").is_none());
    }

    #[test]
    fn a_ready_part_has_no_reason() {
        let ready = serde_json::json!({ "state": "ready" });
        let line = status_response(
            1,
            &ready,
            &ready,
            &serde_json::json!({}),
            "m",
            "configured",
            Some(true),
            None,
            None,
            None,
        );
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["database"], serde_json::json!({ "state": "ready" }));
        assert_eq!(v["construct"]["state"], "ready");
        assert!(v["construct"].get("reason").is_none());
        assert_eq!(v["service"], "registered");
    }
}
