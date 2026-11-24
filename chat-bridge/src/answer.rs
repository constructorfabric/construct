use std::ffi::{c_char, c_void};
use std::io;
use std::sync::{Arc, OnceLock};

use chat_core::chat::{Chat, Deadlines, event_error};
use chat_core::model::{Model, ModelSource};
use chat_core::recall::{self, Facts, ProfileRead};
use chat_core::records::{Records, SessionInfo};
use chat_core::tool::{ChatTools, ToolConfig, ToolHandler};
use connectors::Record;
use serde::Deserialize;
use serde_json::value::RawValue;
use tokio::runtime::Builder;
use tokio_util::sync::CancellationToken;

use super::{INTERNAL_ERROR, argument, c_string, cf_chat_free, guarded};

const EVENT: i32 = 0;
const RECORD: i32 = 1;
const PHONE_TOOLS: &str = include_str!("../../tools/phone.json");
static TOOLS: OnceLock<Result<ToolConfig, String>> = OnceLock::new();

type Callback = extern "C" fn(*mut c_void, i32, *const c_char) -> i32;

#[derive(Clone, Copy)]
struct Output {
    // Only the thread driving the answer calls back. The address is stored as a number so
    // the adapter can satisfy Records: Send without making any raw pointer Send.
    context: usize,
    callback: Callback,
}

impl Output {
    fn write(self, kind: i32, text: &str) -> bool {
        let text = c_string(text);
        let ok = (self.callback)(self.context as *mut c_void, kind, text) != 0;
        // SAFETY: the pointer is ours and the callback only borrows it.
        unsafe { cf_chat_free(text) };
        ok
    }
}

#[derive(Deserialize)]
struct Answer {
    model: Model,
    profile: Box<RawValue>,
    history: Vec<Record>,
    request: Box<RawValue>,
}

struct Settings(Model);

impl ModelSource for Settings {
    fn model(&self) -> Model {
        self.0.clone()
    }
}

struct CopyFacts(Box<RawValue>);

impl Facts for CopyFacts {
    fn enabled(&self) -> bool {
        true
    }

    fn profile<'a>(&'a self, _: &'a reqwest::Client) -> ProfileRead<'a> {
        Box::pin(async { Ok(Some(self.0.get().as_bytes().to_vec())) })
    }
}

impl ToolHandler for CopyFacts {
    fn name(&self) -> &'static str {
        "recall_facts"
    }

    fn run(&self, _: &str) -> Result<String, String> {
        recall::render(self.0.get().as_bytes())
            .map(|text| text.unwrap_or_else(|| "No facts.".into()))
            .map_err(|_| "the facts could not be read".into())
    }
}

struct CopyRecords {
    records: Vec<Record>,
    output: Output,
}

impl Records for CopyRecords {
    fn read_session(&self, source: &str, session: &str) -> io::Result<Vec<Record>> {
        Ok(self
            .records
            .iter()
            .filter(|r| r.tool() == source && r.session() == session)
            .cloned()
            .collect())
    }

    fn append(&mut self, record: &Record) -> io::Result<bool> {
        let line = serde_json::to_string(record)?;
        if !self.output.write(RECORD, &line) {
            return Err(io::Error::other("cannot write the copy"));
        }
        self.records.push(record.clone());
        Ok(true)
    }

    fn flush_session(&mut self, _: &str, _: &str) -> io::Result<()> {
        // The callback commits each record before returning.
        Ok(())
    }

    fn list_sessions(&self, _: &str) -> io::Result<Vec<SessionInfo>> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn delete_session(&mut self, _: &str, _: &str) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn delete_source(&mut self, _: &str) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

fn send(
    input: &str,
    stop: CancellationToken,
    output: Output,
    deadlines: Deadlines,
) -> Result<(), String> {
    let Answer {
        model,
        profile,
        history,
        request,
    } = serde_json::from_str(input).map_err(|_| "invalid request".to_owned())?;
    let config = TOOLS
        .get_or_init(|| {
            ToolConfig::parse(PHONE_TOOLS).map_err(|_| "invalid phone tool file".into())
        })
        .as_ref()
        .map_err(Clone::clone)?;
    let facts = Arc::new(CopyFacts(profile));
    let handlers: [Arc<dyn ToolHandler>; 1] = [facts.clone()];
    let tools = ChatTools::configured(config, &handlers)
        .map_err(|_| "invalid phone tool file".to_owned())?;
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| INTERNAL_ERROR.to_owned())?;
    let chat = Chat::new(
        Arc::new(Settings(model)),
        Box::new(CopyRecords {
            records: history,
            output,
        }),
        runtime.handle().clone(),
        deadlines,
    )
    .map_err(|_| INTERNAL_ERROR.to_owned())?
    .with_facts(facts)
    .with_tools(tools);

    // Runtime::block_on drives the network and timers of this single-threaded runtime. The
    // record and event callbacks run on the host thread that entered cf_chat_send.
    runtime
        .block_on(chat.send_on_runtime(request.get(), stop, &mut |line| output.write(EVENT, line)));
    Ok(())
}

/// Allocate one stop token. Release it only after the send and every stop call have returned.
#[unsafe(no_mangle)]
pub extern "C" fn cf_chat_stop_new() -> *mut CancellationToken {
    Box::into_raw(Box::new(CancellationToken::new()))
}

/// # Safety
/// `stop` is a live token from `cf_chat_stop_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cf_chat_stop_answer(stop: *const CancellationToken) {
    // SAFETY: the caller keeps the token alive for this call.
    if let Some(stop) = unsafe { stop.as_ref() } {
        stop.cancel();
    }
}

/// # Safety
/// `stop` is a live token from `cf_chat_stop_new`, no longer in use.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cf_chat_stop_free(stop: *mut CancellationToken) {
    if !stop.is_null() {
        // SAFETY: the caller transfers the allocation back exactly once.
        drop(unsafe { Box::from_raw(stop) });
    }
}

/// # Safety
/// `input` is a UTF-8 C string and `stop` is a live token. The callback and its context must
/// stay valid until this returns. The callback must not unwind or retain the text pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cf_chat_send(
    input: *const c_char,
    stop: *const CancellationToken,
    callback: Option<Callback>,
    context: *mut c_void,
) {
    let Some(callback) = callback else {
        return;
    };
    let output = Output {
        context: context as usize,
        callback,
    };
    // SAFETY: the caller keeps the string and token alive.
    let request = unsafe { argument(input).zip(stop.as_ref()) };
    let result = guarded(
        || match request {
            Some((input, stop)) => send(&input, stop.clone(), output, Deadlines::default()),
            None => Err("invalid request".into()),
        },
        || Err(INTERNAL_ERROR.into()),
    );
    if let Err(reason) = result {
        output.write(EVENT, &event_error(&reason));
    }
}
