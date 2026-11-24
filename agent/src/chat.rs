//! The chat of the desktop agent: the loop of the `chat-core` crate over the agent's own files.
//!
//! The loop, the wire format, the facts text and the tool trait are the crate's. Here are what only
//! the agent has: the store of the conversations (`FileSink` under the data directory), the facts
//! from the Construct host (see [`Link`]), the tools over the workspace folder, the setup session
//! and the erasure of the person's data.

use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};

use chat_core::model::{Model, ModelSource, read_capped};
use chat_core::recall::{BODY_LIMIT, Facts, ProfileRead};
use chat_core::records::{Records, SessionInfo};
use connectors::Record;
use reqwest::StatusCode;
use tokio::runtime::Handle;

pub use chat_core::chat::{
    Counter, Deadlines, INSTRUCTION, MAX_REQUEST, SOURCE, ToolCount, Usage, WRITE_TIMEOUT,
    event_error,
};

use crate::host::Connection;
use crate::records::{FileSink, Sink};
use crate::settings::Settings;
use crate::sync::lock;

mod bash;
pub mod dashboard_view;
pub mod files;
pub mod machine;
pub mod setup;
pub mod subagent;
pub mod tool;

use tool::ChatTools;

/// Where the chat finds the Construct host: empty until the host is ready. Clones share it.
#[derive(Clone, Default)]
pub struct Link(Arc<Mutex<Option<Connection>>>);

impl Link {
    pub fn set(&self, connection: Connection) {
        *lock(&self.0) = Some(connection);
    }

    pub(crate) fn get(&self) -> Option<Connection> {
        lock(&self.0).clone()
    }
}

impl ModelSource for Settings {
    fn model(&self) -> Model {
        Settings::model(self)
    }
}

/// The facts of the person as the Construct host gives them, while the collection switch is on.
struct HostFacts {
    settings: Settings,
    link: Link,
}

impl Facts for HostFacts {
    fn enabled(&self) -> bool {
        self.settings.collection().enabled
    }

    fn profile<'a>(&'a self, client: &'a reqwest::Client) -> ProfileRead<'a> {
        Box::pin(async move {
            let Some(connection) = self.link.get() else {
                return Ok(None);
            };
            let response = client
                .get(connection.profile_url())
                .bearer_auth(&connection.token)
                .send()
                .await
                .map_err(|_| "unreachable".to_string())?;
            let code = response.status();
            if code != StatusCode::OK {
                return Err(format!("status {}", code.as_u16()));
            }
            read_capped(response, BODY_LIMIT)
                .await
                .map(Some)
                .map_err(|_| "unreadable".to_string())
        })
    }
}

/// The file store of the agent, shared with the readers that set its observer and recorder.
struct SharedSink(Arc<Mutex<FileSink>>);

impl Records for SharedSink {
    fn read_session(&self, source: &str, session: &str) -> io::Result<Vec<Record>> {
        lock(&self.0).read_session(source, session)
    }

    fn list_sessions(&self, source: &str) -> io::Result<Vec<SessionInfo>> {
        lock(&self.0).list_sessions(source)
    }

    fn append(&mut self, record: &Record) -> io::Result<bool> {
        Sink::append(&mut *lock(&self.0), record)
    }

    fn flush_session(&mut self, source: &str, session: &str) -> io::Result<()> {
        lock(&self.0).flush_session(source, session)
    }

    fn delete_session(&mut self, source: &str, session: &str) -> io::Result<()> {
        lock(&self.0).delete_session(source, session)
    }

    fn delete_source(&mut self, source: &str) -> io::Result<()> {
        lock(&self.0).delete_source(source)
    }
}

/// The chat commands. Cheap to clone.
#[derive(Clone)]
pub struct Chat {
    core: chat_core::Chat,
    settings: Settings,
    sink: Arc<Mutex<FileSink>>,
    /// The tools of the setup session, and of a sub-agent of it.
    setup_tools: ChatTools,
    setup_sub_tools: ChatTools,
}

impl Chat {
    /// The chat over the settings, storing its conversations under `data_dir`. Answers run on
    /// `handle`'s runtime, driven by the thread of the connection.
    ///
    /// # Errors
    /// Fails when the HTTP client cannot be built.
    pub fn new(
        settings: Settings,
        data_dir: &Path,
        handle: Handle,
        deadlines: Deadlines,
    ) -> Result<Self, reqwest::Error> {
        let sink = Arc::new(Mutex::new(FileSink::new(data_dir)));
        let core = chat_core::Chat::new(
            Arc::new(settings.clone()),
            Box::new(SharedSink(Arc::clone(&sink))),
            handle,
            deadlines,
        )?
        .with_facts(Arc::new(HostFacts {
            settings: settings.clone(),
            link: Link::default(),
        }));
        Ok(Self {
            core,
            settings,
            sink,
            setup_tools: ChatTools::default(),
            setup_sub_tools: ChatTools::default(),
        })
    }

    /// Read at most `limit` bytes from the provider for one answer; for the tests, since the
    /// default is the crate's. Call it before the chat is cloned.
    #[must_use]
    pub fn with_wire_limit(mut self, limit: usize) -> Self {
        self.core = self.core.with_wire_limit(limit);
        self
    }

    /// Read the facts from the host `link` names; without it every answer starts with no facts.
    /// Call it before the chat is cloned.
    #[must_use]
    pub fn with_link(mut self, link: Link) -> Self {
        self.core = self.core.with_facts(Arc::new(HostFacts {
            settings: self.settings.clone(),
            link,
        }));
        self
    }

    /// Tell `observer` of every user message the chat stores, after it is stored. Call it before
    /// the chat is cloned.
    #[must_use]
    pub fn with_observer(self, observer: crate::follow::Observer) -> Self {
        lock(&self.sink).set_observer(observer);
        self
    }

    /// Tell `recorder` of every record the chat stores, after it is stored. Call it before the
    /// chat is cloned.
    #[must_use]
    pub fn with_recorder(self, recorder: crate::records::Recorder) -> Self {
        lock(&self.sink).set_recorder(recorder);
        self
    }

    /// Offer `tools` to the model and run its calls; without any, no list is sent and every call is
    /// answered with an unknown-tool error. Call it before the chat is cloned.
    #[must_use]
    pub fn with_tools(mut self, tools: ChatTools) -> Self {
        self.core = self.core.with_tools(tools);
        self
    }

    /// Tell `report` the bytes an answer sent and received once it ends, a stopped or failed one
    /// too, with those of its sub-agents counted in. Call it before the chat is cloned.
    #[must_use]
    pub fn with_usage(mut self, report: impl Fn(Usage) + Send + Sync + 'static) -> Self {
        self.core = self.core.with_usage(report);
        self
    }

    /// Run `chat-send` with `argument` (a JSON object). Every line to the client goes through
    /// `out`, which returns `false` when the client is gone.
    pub fn send(&self, argument: &str, out: &mut dyn FnMut(&str) -> bool) {
        self.core.send(argument, out);
    }

    /// Stop every call of `conversation`, and no call of another. `true` when one was running.
    #[must_use]
    pub fn stop(&self, conversation: &str) -> bool {
        self.core.stop(conversation)
    }

    /// The JSON line for `chat-list`: the conversations, the newest first.
    #[must_use]
    pub fn list(&self) -> String {
        self.core.list()
    }

    /// The JSON lines for `chat-get`, one per record, oldest first.
    #[must_use]
    pub fn get(&self, conversation: &str) -> Vec<String> {
        self.core.get(conversation)
    }

    /// Every stored conversation with its records, the newest first.
    ///
    /// # Errors
    /// When the conversations cannot be read.
    pub fn conversations(&self) -> io::Result<Vec<(String, Vec<Record>)>> {
        self.core.conversations()
    }

    /// The line the chat shows for a call to the tool `name`.
    #[must_use]
    pub fn tool_line(&self, name: &str, arguments: &str) -> String {
        self.core.tool_line(name, arguments)
    }

    /// The JSON line for `chat-delete` of one conversation or `all`. A conversation that is
    /// answering is not deleted.
    #[must_use]
    pub fn delete(&self, which: &str) -> String {
        self.core.delete(which)
    }
}

impl crate::erase::OwnStore for Chat {
    fn name(&self) -> &'static str {
        "the chat conversations"
    }

    fn erase(&self) -> Result<(), String> {
        self.core.erase_all()
    }
}
