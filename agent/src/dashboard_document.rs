//! The dashboard document the page shows: read from the workspace, checked against its type and
//! read again when the file changes.
//!
//! The chat edits the file with the file tools and the page may write it too, so the agent
//! looks at it by timer, every [`POLL_INTERVAL`]. The workspace replaces a file in one step, but
//! an editor of the user's may write it in place, so after the first look a text that differs
//! from the last one is used only when two passes in a row read the same: a write seen half way
//! is not judged. A file that changes at every pass is not judged until it stops, and the last
//! good document stays meanwhile. A document that does not pass is not used: the last good one
//! stays and the error is kept, with the place in the file. A missing file is made again from
//! the default. The text of the document is never logged, and no value from it is part of an
//! error.

use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::dashboard::{ACTIVITY_FILE, ensure_default, validate_activity};
use crate::dashboard_projects::Edit;
use crate::sync::lock;
use crate::workspace::{Error, Workspace};

/// Pause between two looks at the file. A change is used after two of them.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// The longest file the agent reads: far more than a page of widgets needs.
pub const MAX_BYTES: u64 = 1024 * 1024;
/// How many times an edit starts over when the file changes under it.
const EDIT_TRIES: usize = 5;

#[derive(Default)]
struct State {
    /// The last document that passed.
    document: Option<Value>,
    /// Why the file as it is now is not used.
    error: Option<Refusal>,
    /// The last text judged; `None` when the last read failed.
    judged: Option<String>,
    /// The text of the last pass, not judged yet.
    seen: Option<String>,
}

/// What the page is told about a file that is not used.
struct Refusal {
    /// What `place` is: `file` (the file as a whole, `place` is empty), `line` (a line and column
    /// of the text) or `pointer` (a JSON pointer into the document; empty is the document itself).
    kind: &'static str,
    place: String,
    message: String,
}

struct Inner {
    workspace: Arc<Workspace>,
    state: Mutex<State>,
    /// Held while an edit reads the file and writes it, so two edits never start from one text.
    editing: Mutex<()>,
}

/// The current dashboard document and the error of the file, shared between the watcher and the
/// socket answer.
#[derive(Clone)]
pub struct Dashboard(Arc<Inner>);

impl Dashboard {
    /// Reads the document once, at once, and returns the holder. A file that does not pass leaves
    /// no document and the error.
    #[must_use]
    pub fn open(workspace: Arc<Workspace>) -> Self {
        let dashboard = Self(Arc::new(Inner {
            workspace,
            state: Mutex::new(State::default()),
            editing: Mutex::new(()),
        }));
        dashboard.guarded(|| dashboard.pass(true));
        dashboard
    }

    /// Looks at the file once.
    pub fn poll(&self) {
        self.pass(false);
    }

    /// Brings the state level with the file as it is now, without waiting for the watcher: a
    /// change is judged on the second identical read, so this looks twice. For a reader that must
    /// see what was just written.
    pub fn settle(&self) {
        self.guarded(|| {
            self.poll();
            self.poll();
        });
    }

    /// Looks at the file every [`POLL_INTERVAL`] on a thread of its own, for as long as the
    /// agent runs.
    pub fn watch(&self) {
        self.watch_every(POLL_INTERVAL);
    }

    /// [`Self::watch`] with another pause between two looks.
    pub fn watch_every(&self, interval: Duration) {
        let dashboard = self.clone();
        thread::spawn(move || {
            loop {
                thread::sleep(interval);
                dashboard.guarded(|| dashboard.poll());
            }
        });
    }

    /// Runs one pass. The code of the passes is the agent's own, but the parser and the schema
    /// checker are other people's: a panic in one of them would end the watcher unseen, with the
    /// answer keeping the last document as if it were current, or at the start take the agent
    /// down. The page is told instead, and the next pass tries again.
    fn guarded(&self, pass: impl FnOnce()) {
        if panic::catch_unwind(AssertUnwindSafe(pass)).is_err() {
            let mut state = lock(&self.0.state);
            state.error = Some(Refusal {
                kind: "file",
                place: String::new(),
                message: "the dashboard reader failed and tries again".to_owned(),
            });
            state.judged = None;
            state.seen = None;
        }
    }

    fn pass(&self, at_once: bool) {
        let text = match self.read() {
            Ok(text) => text,
            Err(e) => {
                let mut state = lock(&self.0.state);
                state.error = Some(Refusal {
                    kind: "file",
                    place: String::new(),
                    message: format!("the file cannot be read: {e}"),
                });
                state.judged = None;
                state.seen = None;
                return;
            }
        };
        let mut state = lock(&self.0.state);
        if state.judged.as_deref() == Some(text.as_str()) {
            state.seen = None;
            return;
        }
        if !at_once && state.seen.as_deref() != Some(text.as_str()) {
            state.seen = Some(text);
            return;
        }
        state.seen = None;
        match check(&text) {
            Ok(document) => {
                state.document = Some(document);
                state.error = None;
            }
            Err(refusal) => state.error = Some(refusal),
        }
        state.judged = Some(text);
    }

    /// The whole file; a missing one is made again from the default first.
    fn read(&self) -> Result<String, Error> {
        let workspace = &self.0.workspace;
        match workspace.read_whole(ACTIVITY_FILE, MAX_BYTES) {
            // Made again only when the file is still missing a moment later; a writer that
            // creates it in the instant between the two steps loses its first version.
            Err(Error::NotFound) => {
                ensure_default(workspace)?;
                workspace.read_whole(ACTIVITY_FILE, MAX_BYTES)
            }
            other => other,
        }
    }

    /// Changes the projects of the document in the file and reads the file back, so the answer
    /// of [`Self::to_json`] shows the change. The edit starts from the file as it is now, not
    /// from the last good document, so a change made by the chat a moment ago stays, and edits
    /// follow one another. A file that does not pass is not edited: the user fixes it first.
    /// An edit that changes nothing leaves the file as it is, and a result longer than
    /// [`MAX_BYTES`], which the reader would refuse, is not written.
    ///
    /// The file is read again just before it is replaced, and the edit starts over on a text that
    /// changed meanwhile, so a change of the chat made while the edit was worked out is not
    /// thrown away. A change in the instant between that read and the replacement cannot be seen
    /// without a lock on the file, the same as for the file tools of the chat.
    ///
    /// # Errors
    /// A plain text: the file cannot be read, does not pass, is too long with the change, keeps
    /// changing, or cannot be written.
    pub fn edit(&self, edit: &Edit) -> Result<(), String> {
        self.edit_with(edit, || {})
    }

    /// [`Self::edit`], with `between` run after the change is worked out and before the file is
    /// read again, as many times as the edit starts over.
    fn edit_with(&self, edit: &Edit, mut between: impl FnMut()) -> Result<(), String> {
        let _one_at_a_time = lock(&self.0.editing);
        for _ in 0..EDIT_TRIES {
            let text = self
                .read()
                .map_err(|e| format!("the file cannot be read: {e}"))?;
            let mut document = check(&text).map_err(|r| {
                format!(
                    "the dashboard file does not pass its type, so it is not changed: {}",
                    r.message
                )
            })?;
            let before = document.clone();
            edit.apply(&mut document);
            if document == before {
                self.guarded(|| self.pass(true));
                return Ok(());
            }
            let mut updated = serde_json::to_string_pretty(&document)
                .map_err(|_| "the dashboard cannot be written".to_owned())?;
            updated.push('\n');
            if updated.len() as u64 > MAX_BYTES {
                return Err("the change would make the dashboard file too long".to_owned());
            }
            between();
            let again = self
                .read()
                .map_err(|e| format!("the file cannot be read: {e}"))?;
            if again != text {
                continue;
            }
            self.0
                .workspace
                .write(ACTIVITY_FILE, &updated)
                .map_err(|e| format!("the file cannot be written: {e}"))?;
            self.guarded(|| self.pass(true));
            return Ok(());
        }
        Err("the dashboard file keeps changing, so it is not changed".to_owned())
    }

    /// The answer to `dashboard-get`: the last good `document` (`null` when there was none) and,
    /// when the file as it is now does not pass, the `error` with its `kind`, `place` and
    /// `message`.
    #[must_use]
    pub fn to_json(&self) -> Value {
        let state = lock(&self.0.state);
        let mut reply = json!({ "document": state.document });
        if let Some(error) = &state.error {
            reply["error"] = json!({
                "kind": error.kind,
                "place": error.place,
                "message": error.message,
            });
        }
        reply
    }

    /// [`Self::to_json`] as the line the socket sends.
    #[must_use]
    pub fn to_json_line(&self) -> String {
        self.to_json().to_string()
    }
}

/// Parses and checks a text. A syntax error names the line and column in the text as it is in
/// the file, a document that breaks the type names a JSON pointer. A message holds no value of
/// the document; it may name a property the type does not know.
fn check(text: &str) -> Result<Value, Refusal> {
    let document: Value = serde_json::from_str(text).map_err(|e| Refusal {
        kind: "line",
        place: format!("line {}, column {}", e.line(), e.column()),
        message: match e.classify() {
            serde_json::error::Category::Eof => "the file ends too soon".to_owned(),
            _ => "the file is not valid JSON".to_owned(),
        },
    })?;
    validate_activity(&document).map_err(|v| Refusal {
        kind: "pointer",
        place: v.place,
        message: v.message,
    })?;
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dashboard::default_activity;

    fn stand(name: &str) -> (Dashboard, Arc<Workspace>, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("cb-document-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let workspace = Arc::new(Workspace::open(&root).unwrap_or_else(|_| std::process::abort()));
        let _ = ensure_default(&workspace);
        (Dashboard::open(Arc::clone(&workspace)), workspace, root)
    }

    fn file(workspace: &Workspace) -> Value {
        let text = workspace
            .read_whole(ACTIVITY_FILE, MAX_BYTES)
            .unwrap_or_default();
        serde_json::from_str(&text).unwrap_or_default()
    }

    #[test]
    fn a_change_made_while_an_edit_is_worked_out_is_not_thrown_away() {
        let (dashboard, workspace, root) = stand("reread");
        let mut changed = file(&workspace);
        changed["title"] = json!("Changed meanwhile");
        let mut calls = 0;
        let result = dashboard.edit_with(&Edit::Hide("/work/api".to_owned()), || {
            calls += 1;
            // Once: the chat writes the file after the edit read it.
            if calls == 1 {
                let _ = workspace.write(ACTIVITY_FILE, &changed.to_string());
            }
        });
        assert_eq!(result, Ok(()));
        assert_eq!(calls, 2, "the edit starts over once");
        let document = file(&workspace);
        assert_eq!(document["title"], "Changed meanwhile");
        assert_eq!(document["payload"]["hidden_projects"], json!(["/work/api"]));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_that_keeps_changing_is_not_overwritten() {
        let (dashboard, workspace, root) = stand("keeps-changing");
        let mut n = 0;
        let result = dashboard.edit_with(&Edit::Hide("/work/api".to_owned()), || {
            n += 1;
            let mut changed = default_activity();
            changed["title"] = json!(format!("Title {n}"));
            let _ = workspace.write(ACTIVITY_FILE, &changed.to_string());
        });
        assert!(result.is_err_and(|e| e.contains("keeps changing")));
        assert_eq!(n, EDIT_TRIES);
        let document = file(&workspace);
        assert_eq!(document["title"], format!("Title {EDIT_TRIES}"));
        assert_eq!(document["payload"]["hidden_projects"], json!([]));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    #[allow(clippy::panic)] // the panic is what the test needs
    fn a_panic_in_a_pass_is_told_to_the_page_and_the_next_pass_goes_on() {
        let root = std::env::temp_dir().join(format!("cb-document-panic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let workspace = Arc::new(Workspace::open(&root).unwrap_or_else(|_| std::process::abort()));
        let dashboard = Dashboard::open(workspace);
        dashboard.guarded(|| panic!("a checker failed"));
        let answer = dashboard.to_json();
        assert_eq!(answer["error"]["kind"], "file");
        assert!(
            answer["error"]["message"]
                .as_str()
                .is_some_and(|m| m.contains("tries again"))
        );
        assert!(!answer.to_string().contains("a checker failed"));
        // The next pass reads the file again and the error goes.
        dashboard.poll();
        dashboard.poll();
        assert!(dashboard.to_json().get("error").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
