//! The dashboard document the agent keeps current: read, watched, and the last good one kept.
//! Each test starts a socket server with a document in a temporary workspace and asks it with
//! `dashboard-get`, as the page does.

#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use construct_agent::dashboard::{ACTIVITY_FILE, default_activity, ensure_default};
use construct_agent::dashboard_document::Dashboard;
use construct_agent::ipc::{Server, request};
use construct_agent::workspace::Workspace;
use serde_json::{Value, json};

/// How long a change may take to show in the answer.
const WITHIN: Duration = Duration::from_secs(5);

struct Stand {
    workspace: Arc<Workspace>,
    socket: PathBuf,
    root: PathBuf,
}

impl Stand {
    /// A workspace with `text` as the document (the default when `None`) and a server over it.
    fn start(name: &str, text: Option<&str>) -> Self {
        let root = std::env::temp_dir().join(format!("cb-document-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let workspace = Arc::new(Workspace::open(&root.join("data")).unwrap());
        match text {
            Some(text) => workspace.write(ACTIVITY_FILE, text).unwrap(),
            None => {
                ensure_default(&workspace).unwrap();
            }
        }
        let dashboard = Dashboard::open(Arc::clone(&workspace));
        dashboard.watch_every(Duration::from_millis(100));
        let socket = root.join("run/construct/agent.sock");
        let server = Server::bind(&socket).unwrap().with_dashboard(dashboard);
        thread::spawn(move || server.serve());
        Self {
            workspace,
            socket,
            root,
        }
    }

    fn get(&self) -> Value {
        let reply = request(&self.socket, "dashboard-get", Duration::from_secs(5)).unwrap();
        serde_json::from_str(&reply).unwrap()
    }

    fn write(&self, document: &Value) {
        self.workspace
            .write(ACTIVITY_FILE, &document.to_string())
            .unwrap();
    }

    /// The answer, once `ready` holds for it; fails after [`WITHIN`].
    fn wait(&self, ready: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + WITHIN;
        loop {
            let answer = self.get();
            if ready(&answer) {
                return answer;
            }
            assert!(Instant::now() < deadline, "no change in time: {answer}");
            thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn titled(title: &str) -> Value {
    let mut document = default_activity();
    document["title"] = json!(title);
    document
}

#[test]
fn the_answer_has_the_document_and_no_error() {
    let stand = Stand::start("first", None);
    assert_eq!(stand.get(), json!({ "document": default_activity() }));
}

#[test]
fn a_change_of_the_file_shows_in_the_answer() {
    let stand = Stand::start("change", None);
    stand.write(&titled("Week"));
    let answer = stand.wait(|a| a["document"]["title"] == "Week");
    assert!(answer.get("error").is_none(), "{answer}");
}

#[test]
fn a_change_that_breaks_the_type_keeps_the_last_good_document_and_names_the_place() {
    let stand = Stand::start("broken", None);
    let mut broken = titled("Broken");
    broken["payload"]["widgets"][1]["position"]["width"] = json!(0);
    stand.write(&broken);
    let answer = stand.wait(|a| a.get("error").is_some());
    assert_eq!(answer["document"], default_activity());
    assert_eq!(
        answer["error"]["place"],
        "/payload/widgets/1/position/width"
    );
    assert!(answer["error"]["message"].is_string());
}

#[test]
fn a_rule_the_schema_cannot_state_is_refused_with_its_place() {
    let stand = Stand::start("rule", None);
    let mut broken = default_activity();
    broken["payload"]["widgets"][3]["id"] = json!("total");
    stand.write(&broken);
    let answer = stand.wait(|a| a.get("error").is_some());
    assert_eq!(answer["document"], default_activity());
    assert_eq!(answer["error"]["place"], "/payload/widgets/3/id");
}

#[test]
fn text_that_is_not_json_names_the_line_and_never_repeats_the_text() {
    let stand = Stand::start("syntax", None);
    stand
        .workspace
        .write(ACTIVITY_FILE, "{\n  \"title\": secret-value,\n}\n")
        .unwrap();
    let answer = stand.wait(|a| a.get("error").is_some());
    assert_eq!(answer["document"], default_activity());
    assert_eq!(answer["error"]["place"], "line 2, column 12");
    assert_eq!(answer["error"]["kind"], "line");
    assert!(!answer.to_string().contains("secret-value"), "{answer}");
}

#[test]
fn a_fixed_file_replaces_the_error() {
    let stand = Stand::start("fixed", None);
    stand.workspace.write(ACTIVITY_FILE, "{").unwrap();
    stand.wait(|a| a.get("error").is_some());
    stand.write(&titled("Fixed"));
    let answer = stand.wait(|a| a["document"]["title"] == "Fixed");
    assert!(answer.get("error").is_none(), "{answer}");
}

#[test]
fn the_old_text_written_back_clears_the_error() {
    let stand = Stand::start("revert", None);
    stand.workspace.write(ACTIVITY_FILE, "{").unwrap();
    stand.wait(|a| a.get("error").is_some());
    stand.write(&default_activity());
    let answer = stand.wait(|a| a.get("error").is_none());
    assert_eq!(answer["document"], default_activity());
}

#[test]
fn a_missing_file_is_made_again_from_the_default() {
    let stand = Stand::start("missing", Some(&titled("Mine").to_string()));
    std::fs::remove_file(stand.root.join("data/workspace").join(ACTIVITY_FILE)).unwrap();
    let answer = stand.wait(|a| a["document"] == default_activity());
    assert!(answer.get("error").is_none(), "{answer}");
    let back = stand.workspace.read(ACTIVITY_FILE, None, None).unwrap();
    assert!(!back.is_empty());
}

#[test]
fn a_file_that_does_not_pass_at_the_start_leaves_no_document_and_the_error() {
    let stand = Stand::start("start", Some("[]"));
    let answer = stand.get();
    assert_eq!(answer["document"], Value::Null);
    assert_eq!(answer["error"]["place"], "/type");
}

#[test]
fn without_a_dashboard_the_command_answers_an_error() {
    let socket = std::env::temp_dir()
        .join(format!("cb-document-none-{}", std::process::id()))
        .join("construct/agent.sock");
    let server = Server::bind(&socket).unwrap();
    thread::spawn(move || server.serve());
    let reply = request(&socket, "dashboard-get", Duration::from_secs(5)).unwrap();
    assert!(reply.contains("not available"), "{reply}");
    let _ = std::fs::remove_dir_all(socket.parent().unwrap().parent().unwrap());
}

/// A dashboard that is looked at by hand: no watcher, so each `poll` is one look at the file.
fn by_hand(name: &str) -> (Dashboard, Arc<Workspace>, PathBuf) {
    let root = std::env::temp_dir().join(format!("cb-document-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let workspace = Arc::new(Workspace::open(&root).unwrap());
    ensure_default(&workspace).unwrap();
    let dashboard = Dashboard::open(Arc::clone(&workspace));
    let file = root.join("workspace").join(ACTIVITY_FILE);
    (dashboard, workspace, file)
}

fn title(dashboard: &Dashboard) -> Value {
    dashboard.to_json()["document"]["title"].clone()
}

#[test]
fn two_quick_changes_end_on_the_last_one_and_the_first_is_never_used() {
    let (dashboard, workspace, _) = by_hand("quick");
    let write = |name: &str| {
        workspace
            .write(ACTIVITY_FILE, &titled(name).to_string())
            .unwrap();
    };
    write("One");
    dashboard.poll();
    // The second change lands before the first was confirmed: "One" is never the document.
    write("Two");
    dashboard.poll();
    assert_eq!(title(&dashboard), "Activity");
    dashboard.poll();
    assert_eq!(title(&dashboard), "Two");
    dashboard.poll();
    assert_eq!(title(&dashboard), "Two");
}

#[test]
fn a_file_seen_half_written_is_not_judged_until_it_stops_changing() {
    let (dashboard, _, file) = by_hand("half");
    let text = serde_json::to_string_pretty(&titled("Whole")).unwrap();
    std::fs::write(&file, &text[..text.len() / 2]).unwrap();
    dashboard.poll();
    // Seen once: no error yet, and the last good document stays.
    assert!(dashboard.to_json().get("error").is_none());
    assert_eq!(title(&dashboard), "Activity");
    std::fs::write(&file, &text).unwrap();
    dashboard.poll();
    assert!(dashboard.to_json().get("error").is_none());
    dashboard.poll();
    assert_eq!(title(&dashboard), "Whole");
    assert!(dashboard.to_json().get("error").is_none());
}

#[test]
fn a_text_that_stays_broken_for_two_looks_is_refused() {
    let (dashboard, _, file) = by_hand("stays");
    std::fs::write(&file, "{").unwrap();
    dashboard.poll();
    assert!(dashboard.to_json().get("error").is_none());
    dashboard.poll();
    assert_eq!(dashboard.to_json()["error"]["kind"], "line");
}

#[test]
fn the_line_and_column_are_those_of_the_file() {
    let (dashboard, _, file) = by_hand("position");
    std::fs::write(&file, "{\n  \"a\": 1\n").unwrap();
    dashboard.poll();
    dashboard.poll();
    let answer = dashboard.to_json();
    assert_eq!(answer["error"]["place"], "line 3, column 0");
    assert_eq!(answer["error"]["message"], "the file ends too soon");
}

#[test]
fn an_error_at_the_root_has_an_empty_pointer() {
    let (dashboard, _, file) = by_hand("root");
    let mut document = default_activity();
    document.as_object_mut().unwrap().remove("title");
    std::fs::write(&file, document.to_string()).unwrap();
    dashboard.poll();
    dashboard.poll();
    let answer = dashboard.to_json();
    assert_eq!(answer["error"]["kind"], "pointer");
    assert_eq!(answer["error"]["place"], "");
    assert_eq!(answer["document"], default_activity());
}

#[test]
fn a_file_that_cannot_be_read_is_an_error_of_the_file_and_the_good_document_stays() {
    let (dashboard, _, file) = by_hand("unreadable");
    std::fs::write(&file, [0xff, 0xfe, 0x41]).unwrap();
    dashboard.poll();
    let answer = dashboard.to_json();
    assert_eq!(answer["error"]["kind"], "file");
    assert_eq!(answer["error"]["place"], "");
    assert_eq!(answer["document"], default_activity());
}

#[test]
fn a_file_over_the_limit_is_refused() {
    let (dashboard, _, file) = by_hand("large");
    let size = usize::try_from(construct_agent::dashboard_document::MAX_BYTES).unwrap() + 1;
    std::fs::write(&file, vec![b' '; size]).unwrap();
    dashboard.poll();
    let answer = dashboard.to_json();
    assert_eq!(answer["error"]["kind"], "file");
    assert!(
        answer["error"]["message"]
            .as_str()
            .unwrap()
            .contains("too large"),
        "{answer}"
    );
    assert_eq!(answer["document"], default_activity());
}
