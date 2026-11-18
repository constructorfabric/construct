//! The dashboard setup as one background job of the agent: it writes the session files, runs the
//! setup session over them, keeps what the session found as facts and tells the activity time to
//! use it. The window starts it and looks at it with `dashboard-setup-start`,
//! `dashboard-setup-status` and `dashboard-setup-cancel`; the job runs on a thread of the agent,
//! so it goes on when the window closes and its state is there when the window opens again.
//!
//! One run at a time. The steps of a run are `writing_files` (the session files of the period),
//! `reading_and_building` (the session reads them and writes the findings, the report and the
//! dashboard; the findings are then kept as facts and the activity time is told to read them) and
//! the ends `done`, `failed` and `cancelled`. A run ends `cancelled` when the person cancels it and
//! when collection is switched off while it goes, at any of its steps.
//!
//! A run that ends any way but `done` leaves the dashboard, the findings and the report as they
//! were before it. They are kept aside in the data directory while the run goes, so that a run the
//! agent did not live to end is undone the same way at the next start, which also removes the
//! session files. When the files cannot all be put back, the run is `failed` with that reason and
//! the copy stays aside for the next start. A run that is `done` keeps what the person changed in
//! the dashboard before it (see [`merge`]); the dashboard it leaves, kept in the data directory
//! only when the run is `done`, is what the next run measures those changes from. The session writes the dashboard in place while it goes, so the page shows
//! what is built so far, and a change the person makes meanwhile is not kept: the run rebuilds
//! the page from the state at its start.
//!
//! The state of the last run is kept in a file, written at each step. The answers hold counts and
//! states, the report the session wrote and fixed texts; never the text of a session file.

mod facts;
mod merge;

use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::OffsetDateTime;

use crate::activity::Feed;
use crate::chat::setup::{CONVERSATION, Outcome};
use crate::chat::{Chat, Counter};
use crate::dashboard::{ACTIVITY_FILE, validate_activity};
use crate::dashboard_document::MAX_BYTES;
use crate::host::write_private;
use crate::ipc::error_response;
use crate::manage::Manager;
use crate::model::Target;
use crate::records::FileSink;
use crate::session_files::{self, SessionFiles};
use crate::settings::{Settings, TOOLS};
use crate::sync::lock;
use crate::workspace::{Error as WorkspaceError, Workspace};

/// The file of the last run's state, in the data directory.
pub const STATE_FILE: &str = "setup-run.json";
/// The dashboard the last finished run left, in the data directory.
const BASELINE_FILE: &str = "setup-baseline.json";
/// What a run that goes would undo, in the data directory.
const BEFORE_FILE: &str = "setup-before.json";
/// What the session writes, in the workspace.
const FINDINGS_FILE: &str = "findings.json";
const REPORT_FILE: &str = "setup-report.md";
/// The most of the findings and the report that is read.
const MAX_FINDINGS: u64 = 1024 * 1024;
const MAX_REPORT: u64 = 64 * 1024;
/// How long a run waits for a tool call that is still running when the session has ended.
const TOOLS_SETTLE: Duration = Duration::from_secs(10);
/// How often the collection switch is looked at while the session runs.
const WATCH_EVERY: Duration = Duration::from_millis(200);

/// Where a run is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// No run has been started yet.
    #[default]
    Idle,
    WritingFiles,
    ReadingAndBuilding,
    Done,
    Failed,
    Cancelled,
}

impl Step {
    /// The word `dashboard-setup-status` answers.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::WritingFiles => "writing_files",
            Self::ReadingAndBuilding => "reading_and_building",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Whether a run is going.
    #[must_use]
    pub fn running(self) -> bool {
        matches!(self, Self::WritingFiles | Self::ReadingAndBuilding)
    }
}

/// What is kept of a run.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Run {
    step: Step,
    /// The session files written, and the bytes of text in them.
    sessions: u64,
    text_bytes: u64,
    /// The bytes sent to the model and received from it; while the run goes they are read from the
    /// meter.
    sent: u64,
    received: u64,
    error: Option<String>,
    /// How many facts the host stored, and why it did not take them all.
    facts_stored: Option<u64>,
    facts_error: Option<String>,
}

/// The arguments of `dashboard-setup-start`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct StartRequest {
    /// The period of the sessions to read, in days.
    days: Option<u32>,
}

/// What a run changes in the workspace, as it was before the run.
#[derive(Serialize, Deserialize)]
struct Before {
    dashboard: Option<String>,
    findings: Option<String>,
    report: Option<String>,
}

impl Before {
    /// The files as they are, a missing one as none. A file that is there and cannot be read (it
    /// is too long, or not text) cannot be put back as it was, so the run does not start.
    fn take(workspace: &Workspace) -> Result<Self, ()> {
        let read = |path: &str| match workspace.read_whole(path, MAX_BYTES) {
            Ok(text) => Ok(Some(text)),
            Err(WorkspaceError::NotFound) => Ok(None),
            Err(_) => Err(()),
        };
        Ok(Self {
            dashboard: read(ACTIVITY_FILE)?,
            findings: read(FINDINGS_FILE)?,
            report: read(REPORT_FILE)?,
        })
    }

    /// Puts the files back as they were; whether all are back.
    fn restore(&self, workspace: &Workspace) -> bool {
        let mut all = true;
        for (path, text) in [
            (ACTIVITY_FILE, &self.dashboard),
            (FINDINGS_FILE, &self.findings),
            (REPORT_FILE, &self.report),
        ] {
            let done = match text {
                Some(text) => workspace.write(path, text),
                None => workspace.remove_all(path),
            };
            if done.is_err() {
                eprintln!("setup: a file could not be put back");
                all = false;
            }
        }
        all
    }

    /// Keeps the files aside, for the user only.
    fn keep(&self, data_dir: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string(self).map_err(std::io::Error::other)?;
        write_private(&data_dir.join(BEFORE_FILE), text.as_bytes())
    }

    fn load(data_dir: &Path) -> Option<Self> {
        let text = fs::read_to_string(data_dir.join(BEFORE_FILE)).ok()?;
        serde_json::from_str(&text).ok()
    }
}

/// How a run ended.
enum End {
    Done,
    Cancelled,
    Failed(String),
}

struct Inner {
    settings: Settings,
    chat: Chat,
    workspace: Arc<Workspace>,
    data_dir: PathBuf,
    feed: Option<Feed>,
    facts: Option<Manager>,
    run: Mutex<Run>,
    /// The count of the run that goes.
    meter: Mutex<Counter>,
    cancel: AtomicBool,
    /// Called as each step begins; the tests hold a run there.
    on_step: Option<Arc<dyn Fn(Step) + Send + Sync>>,
}

/// The setup job. Cheap to clone.
#[derive(Clone)]
pub struct SetupJob {
    inner: Arc<Inner>,
}

impl SetupJob {
    /// The job over the chat that runs the session and the workspace it writes. The state of the
    /// last run is read from `data_dir`. A run that was going is `failed`: what it had changed is
    /// put back and the session files are removed.
    #[must_use]
    pub fn open(
        settings: Settings,
        chat: Chat,
        workspace: Arc<Workspace>,
        data_dir: &Path,
    ) -> Self {
        let mut run: Run = fs::read_to_string(data_dir.join(STATE_FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let job = Self {
            inner: Arc::new(Inner {
                settings,
                chat,
                workspace,
                data_dir: data_dir.to_owned(),
                feed: None,
                facts: None,
                run: Mutex::new(Run::default()),
                meter: Mutex::new(Counter::default()),
                cancel: AtomicBool::new(false),
                on_step: None,
            }),
        };
        // Files kept aside are put back whatever the state says: a run that ended could not put
        // them back, or the agent stopped before it did.
        if let Some(before) = Before::load(data_dir)
            && before.restore(&job.inner.workspace)
        {
            let _ = fs::remove_file(data_dir.join(BEFORE_FILE));
        }
        if run.step.running() {
            if job.inner.workspace.remove_all(session_files::DIR).is_err() {
                eprintln!("setup: the session files could not be removed");
            }
            run.step = Step::Failed;
            run.error = Some("the agent stopped while the setup was running".into());
            job.inner.save(&run);
        }
        *lock(&job.inner.run) = run;
        job
    }

    /// Tell this feed to read the findings when a run is done.
    #[must_use]
    pub fn with_feed(mut self, feed: Feed) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.feed = Some(feed);
        }
        self
    }

    /// Keep the findings as facts through this manager's route when a run is done.
    #[must_use]
    pub fn with_facts(mut self, manager: Manager) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.facts = Some(manager);
        }
        self
    }

    /// Call `hook` as each step of a run begins, on the thread of the run; for the tests.
    #[doc(hidden)]
    #[must_use]
    pub fn with_step_hook(mut self, hook: impl Fn(Step) + Send + Sync + 'static) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.on_step = Some(Arc::new(hook));
        }
        self
    }

    /// The `dashboard-setup-start` command: `argument` is empty or a JSON object with `days`, the
    /// period to read. The answer is the status once the run has started, or an error.
    #[must_use]
    pub fn start(&self, argument: &str) -> String {
        let request = if argument.trim().is_empty() {
            Ok(StartRequest::default())
        } else {
            serde_json::from_str::<StartRequest>(argument)
        };
        let Ok(request) = request else {
            return error_response("the arguments of dashboard-setup-start are not valid");
        };
        let days = match request.days.map(NonZeroU32::new) {
            Some(None) => return error_response("days must be positive"),
            Some(days) => days,
            None => None,
        };
        match self.inner.begin() {
            Ok(()) => {
                let inner = Arc::clone(&self.inner);
                std::thread::spawn(move || inner.execute(days));
                self.status_line()
            }
            Err(message) => error_response(message),
        }
    }

    /// The `dashboard-setup-cancel` command: stops the run that goes, at whichever step it is at.
    /// The run is `cancelled` once it has put the files back, which `dashboard-setup-status`
    /// shows. A fact the host has taken stays taken.
    #[must_use]
    pub fn cancel(&self) -> String {
        if !lock(&self.inner.run).step.running() {
            return error_response("no setup run is going");
        }
        self.inner.cancel.store(true, Ordering::SeqCst);
        // A session that is running ends at once; one that is not yet ends when it starts.
        let _ = self.inner.chat.stop(CONVERSATION);
        self.status_line()
    }

    /// The `dashboard-setup-status` command.
    #[must_use]
    pub fn status(&self) -> Value {
        self.inner.status()
    }

    /// [`Self::status`] as the line the socket sends.
    #[must_use]
    pub fn status_line(&self) -> String {
        self.status().to_string()
    }
}

impl Inner {
    /// Why a run may not start now, or the start of one.
    fn begin(&self) -> Result<(), &'static str> {
        if !self.settings.collects(crate::chat::SOURCE) {
            return Err("collection is off, so the setup does not run");
        }
        if Target::from_settings(&self.settings.model()).is_none() {
            return Err("no model is set: save its address, name and key in the settings");
        }
        let started = {
            let mut run = lock(&self.run);
            if run.step.running() {
                return Err(
                    "a setup run is already going; look at it with dashboard-setup-status or stop it with dashboard-setup-cancel",
                );
            }
            *run = Run {
                step: Step::WritingFiles,
                ..Run::default()
            };
            self.cancel.store(false, Ordering::SeqCst);
            *lock(&self.meter) = Counter::default();
            run.clone()
        };
        self.save(&started);
        Ok(())
    }

    fn status(&self) -> Value {
        let run = lock(&self.run).clone();
        let (sent, received) = if run.step.running() {
            let usage = lock(&self.meter).total();
            (usage.sent, usage.received)
        } else {
            (run.sent, run.received)
        };
        let mut reply = json!({
            "state": run.step.as_str(),
            "sessions": run.sessions,
            "text_bytes": run.text_bytes,
            "sent": sent,
            "received": received,
        });
        if let Some(error) = &run.error {
            reply["error"] = error.clone().into();
        }
        if run.step == Step::Done {
            reply["report"] = self
                .workspace
                .read_whole(REPORT_FILE, MAX_REPORT)
                .unwrap_or_default()
                .into();
        }
        if let Some(stored) = run.facts_stored {
            reply["facts"] = json!({ "stored": stored });
            if let Some(error) = &run.facts_error {
                reply["facts"]["error"] = error.clone().into();
            }
        } else if let Some(error) = &run.facts_error {
            reply["facts"] = json!({ "stored": 0, "error": error });
        }
        reply
    }

    fn save(&self, run: &Run) {
        let written = serde_json::to_string(run)
            .map_err(std::io::Error::other)
            .and_then(|text| write_private(&self.data_dir.join(STATE_FILE), text.as_bytes()));
        if let Err(e) = written {
            eprintln!("setup: the state of the run could not be saved: {e}");
        }
    }

    /// Changes the state and keeps it in its file, outside the lock the answers wait for.
    fn update(&self, change: impl FnOnce(&mut Run)) {
        let run = {
            let mut run = lock(&self.run);
            change(&mut run);
            run.clone()
        };
        self.save(&run);
    }

    /// Whether the run is to end: it was cancelled, or collection was switched off.
    fn stopped(&self) -> bool {
        self.cancel.load(Ordering::SeqCst) || !self.settings.collects(crate::chat::SOURCE)
    }

    fn step(&self, step: Step) {
        self.update(|run| run.step = step);
        if let Some(hook) = &self.on_step {
            hook(step);
        }
    }

    /// The whole run, on its own thread.
    fn execute(&self, days: Option<NonZeroU32>) {
        let Ok(before) = Before::take(&self.workspace) else {
            self.finish(&End::Failed(
                "a file of the workspace could not be read, so the run did not start".into(),
            ));
            return;
        };
        // Without a copy of the files kept aside a run could not be undone, so it does not start.
        let mut end = if let Err(e) = before.keep(&self.data_dir) {
            eprintln!("setup: the files could not be kept aside: {e}");
            End::Failed("the files of the run could not be kept aside".into())
        } else {
            // A panic must not leave the run `going` for ever, with nothing to end it.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.work(days, before.findings.is_some(), &before)
            }));
            if result.is_err() {
                // `work` did not get to wait for the tool calls of the session.
                let _ = self.chat.wait_for_tools(TOOLS_SETTLE);
            }
            result.unwrap_or_else(|_| End::Failed("the run stopped unexpectedly".into()))
        };
        // A run that is done has nothing to put back.
        if matches!(end, End::Done) || before.restore(&self.workspace) {
            let _ = fs::remove_file(self.data_dir.join(BEFORE_FILE));
        } else {
            // The copy stays aside, and the next start puts the files back.
            end = End::Failed(
                "the old files could not all be put back; they are kept aside and put back at the next start of the agent"
                    .into(),
            );
        }
        self.finish(&end);
    }

    /// Records how a run ended.
    fn finish(&self, end: &End) {
        let usage = lock(&self.meter).total();
        self.update(|run| {
            run.sent = usage.sent;
            run.received = usage.received;
            match end {
                End::Done => run.step = Step::Done,
                End::Cancelled => run.step = Step::Cancelled,
                End::Failed(reason) => {
                    run.step = Step::Failed;
                    run.error = Some(reason.clone());
                }
            }
        });
        // The line says how it ended and nothing else.
        eprintln!("setup run: {}", lock(&self.run).step.as_str());
    }

    fn work(&self, days: Option<NonZeroU32>, again: bool, before: &Before) -> End {
        if let Some(hook) = &self.on_step {
            hook(Step::WritingFiles);
        }
        let sources: Vec<&str> = TOOLS
            .into_iter()
            .filter(|tool| self.settings.collects(tool))
            .collect();
        let sink = FileSink::new(&self.data_dir);
        let written = SessionFiles::write(
            &self.workspace,
            &sink,
            &sources,
            days,
            OffsetDateTime::now_utc(),
            &mut |sessions, bytes| {
                // Not saved: the answers read it from here, and a run that dies is `failed`.
                let mut run = lock(&self.run);
                run.sessions = sessions as u64;
                run.text_bytes = bytes;
                drop(run);
                !self.stopped()
            },
        );
        let files = match written {
            Ok(Some(files)) => files,
            Ok(None) => return End::Cancelled,
            Err(_) => return End::Failed("the session files could not be written".into()),
        };
        if files.sessions() == 0 {
            return End::Failed("there is no session to read in this period".into());
        }
        if self.stopped() {
            return End::Cancelled;
        }
        self.step(Step::ReadingAndBuilding);
        if self.stopped() {
            return End::Cancelled;
        }
        let outcome = self.session(again);
        // A stopped session leaves the tool call it waited for running, and what that writes
        // lands later: it is waited for, so that the files are put back after it and the folder
        // that holds the person's text is removed after it.
        let settled = self.chat.wait_for_tools(TOOLS_SETTLE);
        let _ = files.close();
        if !settled {
            return End::Failed(
                "a tool call of the session was still running when the session ended, so a file may have changed after the old ones were put back".into(),
            );
        }
        match outcome {
            Ok(Outcome::Done) if !self.stopped() => {}
            Ok(Outcome::Done | Outcome::Stopped) => return End::Cancelled,
            Err(reason) => return End::Failed(plain_reason(&reason)),
        }
        let left = match self.check_and_keep(before) {
            Ok(left) => left,
            Err(reason) => return End::Failed(reason.into()),
        };
        if !self.keep_facts() {
            return End::Cancelled;
        }
        // Only a run that is done moves the baseline, so that one that is undone leaves it as it
        // was, as it leaves the dashboard.
        self.keep_baseline(&left);
        if let Some(feed) = &self.feed {
            feed.findings_changed();
        }
        End::Done
    }

    /// The setup session. While it runs the collection switch is looked at, and a session that
    /// would send text with collection off is stopped.
    fn session(&self, again: bool) -> Result<Outcome, String> {
        let meter = lock(&self.meter).clone();
        let over = AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                while !over.load(Ordering::SeqCst) {
                    if self.stopped() {
                        let _ = self.chat.stop(CONVERSATION);
                    }
                    std::thread::sleep(WATCH_EVERY);
                }
            });
            let outcome = self.chat.setup_with(again, &meter, &|| self.stopped());
            over.store(true, Ordering::SeqCst);
            outcome
        })
    }

    /// Checks what the session wrote, and puts the person's changes of the dashboard into it.
    /// Answers the text of the dashboard that is left.
    fn check_and_keep(&self, before: &Before) -> Result<String, &'static str> {
        let findings = self
            .workspace
            .read_whole(FINDINGS_FILE, MAX_FINDINGS)
            .map_err(|_| "the session wrote no findings")?;
        if !serde_json::from_str::<Value>(&findings).is_ok_and(|value| value.is_object()) {
            return Err("the findings the session wrote are not a JSON object");
        }
        let dashboard = self
            .workspace
            .read_whole(ACTIVITY_FILE, MAX_BYTES)
            .map_err(|_| "the session left no dashboard")?;
        let document = serde_json::from_str::<Value>(&dashboard)
            .ok()
            .filter(|document| validate_activity(document).is_ok())
            .ok_or("the dashboard the session wrote does not pass its type")?;
        let baseline = fs::read_to_string(self.data_dir.join(BASELINE_FILE)).ok();
        let kept = before.dashboard.as_deref().and_then(|now| {
            // The widgets the person changed may not fit the page the session built: then they
            // are the session's, and the hidden projects and the names are put back alone.
            [true, false].into_iter().find_map(|with_widgets| {
                merge::keep_changes(now, baseline.as_deref(), &document, with_widgets)
                    .filter(|kept| validate_activity(kept).is_ok())
            })
        });
        let text = if let Some(kept) = kept {
            let mut text = serde_json::to_string_pretty(&kept)
                .map_err(|_| "the dashboard could not be written")?;
            text.push('\n');
            if text.len() as u64 > MAX_BYTES {
                return Err("the dashboard would be too long with the person's changes");
            }
            self.workspace
                .write(ACTIVITY_FILE, &text)
                .map_err(|_| "the dashboard could not be written")?;
            text
        } else {
            dashboard
        };
        Ok(text)
    }

    /// What a run that is done leaves is what the next run measures the person's changes from.
    /// A baseline that cannot be written is removed: an old one would tell the next run that the
    /// session's own changes were the person's.
    fn keep_baseline(&self, dashboard: &str) {
        let file = self.data_dir.join(BASELINE_FILE);
        if let Err(e) = write_private(&file, dashboard.as_bytes()) {
            eprintln!("setup: the baseline of the dashboard could not be saved: {e}");
            let _ = fs::remove_file(file);
        }
    }

    /// Sends the findings to the facts route. A host that does not take them does not undo the
    /// run: the reason is kept for the status. Answers `false` when the run was stopped meanwhile.
    fn keep_facts(&self) -> bool {
        let lines = self
            .workspace
            .read_whole(FINDINGS_FILE, MAX_FINDINGS)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .map(|findings| facts::lines(&findings))
            .unwrap_or_default();
        if lines.is_empty() {
            return !self.stopped();
        }
        let Some(manager) = &self.facts else {
            self.update(|run| run.facts_error = Some("facts are not available".into()));
            return !self.stopped();
        };
        let sent = manager.remember(&lines, &|| !self.stopped());
        self.update(|run| {
            run.facts_stored = Some(sent.stored);
            run.facts_error = sent.error;
        });
        !self.stopped()
    }
}

/// The reason of a failed session in words for the person; it never holds what the provider said.
fn plain_reason(reason: &str) -> String {
    match reason {
        "busy" => "an answer of the chat is running, so the setup cannot start now".into(),
        "no model" => "no model is set: save its address, name and key in the settings".into(),
        "no tools" => "the setup has no tools to read and write the files".into(),
        other => format!("the session failed: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reason_of_the_chat_is_said_in_plain_words() {
        assert!(plain_reason("busy").contains("answer of the chat is running"));
        assert!(plain_reason("no model").starts_with("no model is set"));
        assert!(plain_reason("no tools").contains("no tools"));
        assert_eq!(plain_reason("timeout"), "the session failed: timeout");
    }
}
