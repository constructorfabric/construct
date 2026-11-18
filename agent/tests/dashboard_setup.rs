//! The dashboard setup as a background job, through the agent's socket: the commands start it,
//! show its steps and end it; it is refused while another run goes and with collection off; a
//! run that fails or is cancelled leaves the dashboard as it was; a second run keeps what the
//! person changed; the findings are kept as facts; no answer holds the text of a session.
//!
//! The model is a scripted fake provider, the host of Construct a fake that notes its calls, and
//! the sessions are made up.
#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use construct_agent::chat::tool::ChatTool;
use construct_agent::dashboard::ACTIVITY_FILE;
use construct_agent::dashboard_document::Dashboard;
use construct_agent::dashboard_projects::Edit;
use construct_agent::dashboard_setup::{STATE_FILE, Step};
use construct_agent::host::Connection;
use construct_agent::ipc::{Server, request};
use construct_agent::records::FileSink;
use construct_agent::workspace::Workspace;
use serde_json::{Value, json};

mod common;

use common::fake_host::{STORED, TOKEN};
use common::scripted::Turn;
use common::setup_rig::{REPORT, Rig, SECRET, dashboard, findings, run_of, write};
use common::stand::{set_collection, wait_for};

static NEXT: AtomicU32 = AtomicU32::new(0);

struct Stand {
    base: PathBuf,
    socket: PathBuf,
    rig: Rig,
    _rt: tokio::runtime::Runtime,
}

impl Stand {
    fn new(turns: Vec<Turn>) -> Self {
        Self::with(turns, |_| {})
    }

    /// A stand whose data directory `before` may change before the job is opened.
    fn with(turns: Vec<Turn>, before: impl FnOnce(&Path)) -> Self {
        Self::build(turns, before, |_| None)
    }

    /// A stand with one more tool for the session, over the workspace.
    fn build(
        turns: Vec<Turn>,
        before: impl FnOnce(&Path),
        extra: impl FnOnce(&Arc<Workspace>) -> Option<Arc<dyn ChatTool>>,
    ) -> Self {
        let base = std::env::temp_dir().join(format!(
            "cb-setup-job-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let rig = Rig::new(&base, rt.handle().clone(), turns, None, before, extra);
        let server = Server::bind(&base.join("agent.sock"))
            .unwrap()
            .with_setup(rig.job.clone())
            .with_dashboard(Dashboard::open(Arc::clone(&rig.workspace)));
        let socket = server.path().to_path_buf();
        std::thread::spawn(move || {
            let _ = server.serve();
        });
        Self {
            base,
            socket,
            rig,
            _rt: rt,
        }
    }

    fn ask(&self, line: &str) -> Value {
        let reply = request(&self.socket, line, Duration::from_secs(10)).unwrap();
        serde_json::from_str(&reply).unwrap()
    }

    fn status(&self) -> Value {
        self.ask("dashboard-setup-status")
    }

    fn wait_for_state(&self, state: &str) -> Value {
        let mut last = Value::Null;
        wait_for(state, || {
            last = self.status();
            last["state"] == state
        });
        last
    }

    fn hold(&self) -> &common::hold::Hold {
        &self.rig.hold
    }

    fn read(&self, path: &str) -> Option<String> {
        self.rig.read(path)
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// The baseline the last finished run left, as text.
fn baseline(stand: &Stand) -> Option<String> {
    std::fs::read_to_string(stand.rig.data.join("setup-baseline.json")).ok()
}

/// A tool that does not return until it is let go, and then writes the dashboard: what a tool
/// call that a stop did not wait for does.
struct Late {
    workspace: Arc<Workspace>,
    started: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
}

impl ChatTool for Late {
    fn name(&self) -> &'static str {
        "late"
    }

    fn description(&self) -> &'static str {
        "Writes late."
    }

    fn schema(&self) -> &'static str {
        r#"{"type":"object","properties":{}}"#
    }

    fn run(&self, _: &str) -> Result<String, String> {
        self.started.store(true, Ordering::SeqCst);
        while !self.release.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(10));
        }
        self.workspace.write(ACTIVITY_FILE, "late").unwrap();
        Ok("late".into())
    }
}

/// Lets a held tool go when the test ends, however it ends: a failed assertion must not leave the
/// tool blocked, or the runtime would wait for it at shutdown and the failure would be a hang.
struct Release(Arc<AtomicBool>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// An answer must not hold any text of a session.
fn assert_no_session_text(answer: &Value) {
    let text = answer.to_string();
    assert!(!text.contains(SECRET), "{text}");
    assert!(!text.contains("work on the"), "{text}");
}

#[test]
fn the_commands_start_the_run_show_its_steps_and_end_it() {
    let stand = Stand::new(run_of(&dashboard(&["lab"], &json!({ "shop": "PLAT" }))));
    assert_eq!(stand.status()["state"], "idle");

    stand.hold().at(Step::ReadingAndBuilding);
    let started = stand.ask(r#"dashboard-setup-start {"days":3}"#);
    assert_eq!(started["state"], "writing_files", "{started}");
    let reading = stand.wait_for_state("reading_and_building");
    // The files are written: three sessions, and the size of their text.
    assert_eq!(reading["sessions"], 3, "{reading}");
    assert!(reading["text_bytes"].as_u64().unwrap() > 3 * SECRET.len() as u64);
    assert!(reading.get("report").is_none());
    assert_no_session_text(&reading);
    let history = stand.rig.workspace.glob("history/*.txt", "").unwrap();
    assert_eq!(history.len(), 3, "{history:?}");

    stand.hold().release();
    let done = stand.wait_for_state("done");
    assert_eq!(done["sessions"], 3);
    assert!(done["sent"].as_u64().unwrap() > 0, "{done}");
    assert!(done["received"].as_u64().unwrap() > 0, "{done}");
    assert_eq!(done["report"], REPORT);
    assert_no_session_text(&done);
    // The session files went with the run.
    assert!(
        stand
            .rig
            .workspace
            .glob("history/*", "")
            .unwrap()
            .is_empty()
    );
    // The dashboard is the one the session wrote.
    let page: Value = serde_json::from_str(&stand.read(ACTIVITY_FILE).unwrap()).unwrap();
    assert_eq!(page["payload"]["hidden_projects"], json!(["lab"]));
    // What was kept aside for the run is gone.
    assert!(!stand.rig.data.join("setup-before.json").exists());
    // A run that is done cannot be cancelled.
    assert_eq!(
        stand.ask("dashboard-setup-cancel")["error"],
        "no setup run is going"
    );
}

#[test]
fn a_second_start_while_a_run_goes_is_refused_with_a_reason() {
    let stand = Stand::new(run_of(&dashboard(&[], &json!({}))));
    stand.hold().at(Step::ReadingAndBuilding);
    stand.ask("dashboard-setup-start");
    stand.wait_for_state("reading_and_building");

    let refused = stand.ask("dashboard-setup-start");
    let reason = refused["error"].as_str().unwrap();
    assert!(reason.contains("already going"), "{reason}");
    // The run goes on as it was.
    assert_eq!(stand.status()["state"], "reading_and_building");

    stand.hold().release();
    stand.wait_for_state("done");
}

#[test]
fn a_start_is_refused_with_collection_off_no_model_or_a_bad_period() {
    let stand = Stand::new(Vec::new());
    set_collection(&stand.rig.settings, r#"{"enabled":false}"#);
    let refused = stand.ask("dashboard-setup-start");
    assert!(
        refused["error"]
            .as_str()
            .unwrap()
            .contains("collection is off"),
        "{refused}"
    );
    set_collection(&stand.rig.settings, r#"{"enabled":true}"#);
    assert_eq!(
        stand.ask(r#"dashboard-setup-start {"days":0}"#)["error"],
        "days must be positive"
    );
    assert!(
        stand.ask(r#"dashboard-setup-start {"weeks":1}"#)["error"]
            .as_str()
            .unwrap()
            .contains("not valid")
    );
    stand
        .rig
        .settings
        .apply(
            construct_agent::settings::Update::parse(r#"{"model":{"remove_key":true}}"#).unwrap(),
        )
        .unwrap();
    let refused = stand.ask("dashboard-setup-start");
    assert!(
        refused["error"]
            .as_str()
            .unwrap()
            .contains("no model is set"),
        "{refused}"
    );
    // Nothing started, nothing was sent to the model.
    assert_eq!(stand.status()["state"], "idle");
    assert!(stand.rig.bodies().is_empty());
}

/// What a cancelled or failed run must leave: the dashboard as it was, no findings and no report.
fn assert_untouched(stand: &Stand, dashboard_before: &str) {
    assert_eq!(stand.read(ACTIVITY_FILE).as_deref(), Some(dashboard_before));
    assert_eq!(stand.read("findings.json"), None);
    assert_eq!(stand.read("setup-report.md"), None);
    assert!(
        stand
            .rig
            .workspace
            .glob("history/*", "")
            .unwrap()
            .is_empty()
    );
    assert!(!stand.rig.data.join("setup-before.json").exists());
}

#[test]
fn a_cancel_ends_the_run_at_the_writing_of_the_files() {
    let stand = Stand::new(run_of(&dashboard(&["lab"], &json!({}))));
    let before = stand.read(ACTIVITY_FILE).unwrap();
    stand.hold().at(Step::WritingFiles);
    stand.ask("dashboard-setup-start");
    wait_for("the first step", || {
        stand.hold().reached(Step::WritingFiles)
    });
    stand.ask("dashboard-setup-cancel");
    stand.hold().release();
    let end = stand.wait_for_state("cancelled");
    assert_no_session_text(&end);
    assert_untouched(&stand, &before);
    // The model was never asked.
    assert!(stand.rig.bodies().is_empty());
}

#[test]
fn a_cancel_ends_the_run_between_the_files_and_the_session() {
    let stand = Stand::new(run_of(&dashboard(&["lab"], &json!({}))));
    let before = stand.read(ACTIVITY_FILE).unwrap();
    stand.hold().at(Step::ReadingAndBuilding);
    stand.ask("dashboard-setup-start");
    stand.wait_for_state("reading_and_building");
    stand.ask("dashboard-setup-cancel");
    stand.hold().release();
    stand.wait_for_state("cancelled");
    assert_untouched(&stand, &before);
    assert!(stand.rig.bodies().is_empty());
}

#[test]
fn a_cancel_stops_a_session_that_waits_for_the_model() {
    // The model never answers its first request.
    let stand = Stand::new(vec![Turn::Silence]);
    let before = stand.read(ACTIVITY_FILE).unwrap();
    stand.ask("dashboard-setup-start");
    wait_for("the request", || !stand.rig.bodies().is_empty());
    let sent = stand.status()["sent"].as_u64().unwrap();
    assert!(
        sent > 0,
        "the size sent so far is shown while the session runs"
    );
    stand.ask("dashboard-setup-cancel");
    let end = stand.wait_for_state("cancelled");
    assert!(end["sent"].as_u64().unwrap() >= sent);
    assert_untouched(&stand, &before);
}

#[test]
fn a_cancel_after_the_session_wrote_its_files_puts_the_old_ones_back() {
    // The session writes everything, then waits for a model turn that is held back.
    let open = Arc::new(AtomicBool::new(false));
    let mut turns = run_of(&dashboard(&["lab"], &json!({})));
    turns[1] = Turn::Gated(Arc::clone(&open), Box::new(Turn::Text("done")));
    let stand = Stand::new(turns);
    let before = stand.read(ACTIVITY_FILE).unwrap();
    stand.ask("dashboard-setup-start");
    wait_for("the second request", || stand.rig.bodies().len() >= 2);
    assert!(stand.read("findings.json").is_some());
    stand.ask("dashboard-setup-cancel");
    stand.wait_for_state("cancelled");
    assert_untouched(&stand, &before);
    open.store(true, Ordering::SeqCst);
}

#[test]
fn a_cancel_while_the_facts_are_sent_ends_the_run_and_puts_the_old_files_back() {
    let stand = Stand::new(run_of(&dashboard(&["lab"], &json!({}))));
    let before = stand.read(ACTIVITY_FILE).unwrap();
    *stand.rig.host.delay.lock().unwrap() = Duration::from_millis(700);
    stand.ask("dashboard-setup-start");
    wait_for("the call to the host", || stand.rig.host.count() == 1);
    stand.ask("dashboard-setup-cancel");
    stand.wait_for_state("cancelled");
    assert_untouched(&stand, &before);
}

#[test]
fn switching_collection_off_while_the_run_goes_ends_it_before_the_session_sends_anything() {
    let stand = Stand::new(run_of(&dashboard(&["lab"], &json!({}))));
    let before = stand.read(ACTIVITY_FILE).unwrap();
    stand.hold().at(Step::ReadingAndBuilding);
    stand.ask("dashboard-setup-start");
    stand.wait_for_state("reading_and_building");
    set_collection(&stand.rig.settings, r#"{"enabled":false}"#);
    stand.hold().release();
    stand.wait_for_state("cancelled");
    assert_untouched(&stand, &before);
    assert!(stand.rig.bodies().is_empty());
    assert_eq!(stand.rig.host.count(), 0);
}

#[test]
fn a_run_that_fails_leaves_the_old_dashboard_as_it_was() {
    // The session writes a dashboard that does not pass its type.
    let mut broken = dashboard(&["lab"], &json!({}));
    broken["payload"]["widgets"] = json!("none");
    let stand = Stand::new(run_of(&broken));
    let before = stand.read(ACTIVITY_FILE).unwrap();
    stand.ask("dashboard-setup-start");
    let failed = stand.wait_for_state("failed");
    assert!(
        failed["error"]
            .as_str()
            .unwrap()
            .contains("does not pass its type"),
        "{failed}"
    );
    assert_untouched(&stand, &before);
    assert_no_session_text(&failed);
    // Nothing was told to the host.
    assert_eq!(stand.rig.host.count(), 0);
}

#[test]
fn a_session_that_leaves_no_findings_or_findings_that_are_no_object_fails_the_run() {
    let only_dashboard = vec![
        Turn::Calls(vec![write(
            ACTIVITY_FILE,
            &dashboard(&["lab"], &json!({})).to_string(),
        )]),
        Turn::Text("done"),
    ];
    let stand = Stand::new(only_dashboard);
    let before = stand.read(ACTIVITY_FILE).unwrap();
    stand.ask("dashboard-setup-start");
    let failed = stand.wait_for_state("failed");
    assert!(
        failed["error"].as_str().unwrap().contains("no findings"),
        "{failed}"
    );
    assert_untouched(&stand, &before);

    let list = vec![
        Turn::Calls(vec![
            write("findings.json", "[1, 2]"),
            write(ACTIVITY_FILE, &dashboard(&["lab"], &json!({})).to_string()),
        ]),
        Turn::Text("done"),
    ];
    let stand = Stand::new(list);
    let before = stand.read(ACTIVITY_FILE).unwrap();
    stand.ask("dashboard-setup-start");
    let failed = stand.wait_for_state("failed");
    assert!(
        failed["error"]
            .as_str()
            .unwrap()
            .contains("not a JSON object"),
        "{failed}"
    );
    assert_untouched(&stand, &before);
}

#[test]
fn a_model_that_refuses_fails_the_run_with_a_plain_reason() {
    let stand = Stand::new(vec![Turn::Error]);
    let before = stand.read(ACTIVITY_FILE).unwrap();
    stand.ask("dashboard-setup-start");
    let failed = stand.wait_for_state("failed");
    assert!(
        failed["error"]
            .as_str()
            .unwrap()
            .starts_with("the session failed")
    );
    assert_untouched(&stand, &before);
}

#[test]
fn a_period_without_sessions_fails_the_run() {
    let stand = Stand::with(Vec::new(), |data| {
        for tool in ["claude-code", "codex"] {
            FileSink::new(data).delete_source(tool).unwrap();
        }
    });
    stand.ask("dashboard-setup-start");
    let failed = stand.wait_for_state("failed");
    assert!(failed["error"].as_str().unwrap().contains("no session"));
    assert!(stand.rig.bodies().is_empty());
}

#[test]
fn after_a_restart_in_the_middle_of_a_run_the_status_says_so_and_the_old_files_are_back() {
    // The session writes its files, and then waits for a turn that is held back: the agent
    // stops there.
    let open = Arc::new(AtomicBool::new(false));
    let mut turns = run_of(&dashboard(&["lab"], &json!({})));
    turns[1] = Turn::Gated(Arc::clone(&open), Box::new(Turn::Text("done")));
    turns.extend(run_of(&dashboard(&["lab"], &json!({}))));
    let stand = Stand::new(turns);
    let before = stand.read(ACTIVITY_FILE).unwrap();
    stand.ask("dashboard-setup-start");
    wait_for("the second request", || stand.rig.bodies().len() >= 2);
    assert!(stand.read("findings.json").is_some());
    assert_eq!(stand.status()["state"], "reading_and_building");
    assert!(stand.rig.data.join(STATE_FILE).exists());

    // The next start of the agent opens a job over the same directory.
    let restarted = stand.rig.open_job(None);
    let status = restarted.status();
    assert_eq!(status["state"], "failed", "{status}");
    assert!(status["error"].as_str().unwrap().contains("agent stopped"));
    assert_eq!(status["sessions"], 3);
    assert_untouched(&stand, &before);

    // The run that went on in the old process may end as it likes; the new job starts again.
    open.store(true, Ordering::SeqCst);
    wait_for("the old run to end", || {
        stand.status()["state"] != "reading_and_building"
    });
    let again = restarted.start("");
    assert!(!again.contains("error"), "{again}");
}

#[test]
fn a_state_file_a_dead_run_left_reads_as_failed_and_a_new_start_runs_to_the_end() {
    let stand = Stand::with(run_of(&dashboard(&[], &json!({}))), |data| {
        let state = json!({ "step": "writing_files", "sessions": 1, "text_bytes": 5 });
        std::fs::write(data.join(STATE_FILE), state.to_string()).unwrap();
    });
    let status = stand.status();
    assert_eq!(status["state"], "failed", "{status}");
    assert!(status["error"].as_str().unwrap().contains("agent stopped"));

    stand.ask("dashboard-setup-start");
    let done = stand.wait_for_state("done");
    assert_eq!(done["report"], REPORT);
    assert!(done.get("error").is_none());
}

#[test]
fn a_second_run_keeps_a_hidden_project_a_renamed_one_and_a_widget_and_starts_from_the_findings() {
    let first = dashboard(&["lab"], &json!({ "shop": "PLAT" }));
    // The second session writes a page that forgets what the person did meanwhile.
    let second = dashboard(&[], &json!({ "shop": "Shop again" }));
    let mut turns = run_of(&first);
    turns.extend(run_of(&second));
    let stand = Stand::new(turns);
    stand.ask("dashboard-setup-start");
    stand.wait_for_state("done");

    // The person hides the blog and names the lab, which the first run hid, and adds a widget,
    // as the chat does with its file tools.
    let page = Dashboard::open(Arc::clone(&stand.rig.workspace));
    page.edit(&Edit::Hide("blog".into())).unwrap();
    page.edit(&Edit::Rename {
        project: "lab".into(),
        name: "Playground".into(),
    })
    .unwrap();
    let mut document: Value = serde_json::from_str(&stand.read(ACTIVITY_FILE).unwrap()).unwrap();
    document["payload"]["widgets"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "id": "mine", "kind": "table", "group_by": "tool",
            "position": { "column": 0, "row": 14, "width": 12, "height": 3 } }));
    stand
        .rig
        .workspace
        .write(ACTIVITY_FILE, &document.to_string())
        .unwrap();

    stand.ask("dashboard-setup-start");
    stand.wait_for_state("done");
    let page: Value = serde_json::from_str(&stand.read(ACTIVITY_FILE).unwrap()).unwrap();
    let hidden: Vec<&str> = page["payload"]["hidden_projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    // The one the person hid is hidden; the lab, which the session hid before and no longer does,
    // is the session's to change.
    assert!(hidden.contains(&"blog"), "{hidden:?}");
    assert!(!hidden.contains(&"lab"), "{hidden:?}");
    let names = &page["payload"]["project_names"];
    assert_eq!(names["lab"], "Playground");
    // A name the person did not give is the session's.
    assert_eq!(names["shop"], "Shop again");
    // The widget the person added is still there.
    let widgets = page["payload"]["widgets"].as_array().unwrap();
    assert!(widgets.iter().any(|w| w["id"] == "mine"), "{widgets:?}");

    // The second session was told to start from what is there.
    let bodies = stand.rig.bodies();
    let first_message = bodies[0]["messages"].to_string();
    let second_message = bodies[2]["messages"].to_string();
    assert!(!first_message.contains("again"), "{first_message}");
    assert!(
        second_message.contains("Set up my dashboard again"),
        "{second_message}"
    );
}

#[test]
fn the_findings_are_kept_as_facts_through_the_route_of_the_prompts() {
    let stand = Stand::new(run_of(&dashboard(&[], &json!({}))));
    stand.ask("dashboard-setup-start");
    let done = stand.wait_for_state("done");
    assert!(done["facts"]["stored"].as_u64().unwrap() >= 1, "{done}");
    assert!(done["facts"].get("error").is_none(), "{done}");

    let calls = stand.rig.host.calls.lock().unwrap();
    assert!(!calls.is_empty());
    for call in calls.iter() {
        // The route of the prompts: its path and its body.
        assert_eq!(call.method, "POST");
        assert!(call.path.ends_with("/facts/manage"), "{}", call.path);
        assert!(
            call.path.starts_with("/construct/v1/subjects/"),
            "{}",
            call.path
        );
        assert_eq!(call.authorization, format!("Bearer {TOKEN}"));
        let mut members: Vec<&String> = call.body.as_object().unwrap().keys().collect();
        members.sort();
        assert_eq!(members, ["conversation", "message"]);
        assert_eq!(call.body["conversation"], "");
    }
    drop(calls);
    let said = stand.rig.host.messages().join("\n");
    assert!(
        said.contains("I work with the task tracker alpha (issue tracker)."),
        "{said}"
    );
    assert!(said.contains("task tracker beta"), "{said}");
    assert!(
        said.contains("project PLAT of the task tracker alpha"),
        "{said}"
    );
    assert!(
        said.contains("I group my projects PLAT, BLOG together as work."),
        "{said}"
    );
    // What goes to the host is what the session found, not what the sessions said.
    assert!(!said.contains(SECRET));
}

#[test]
fn facts_the_host_took_before_it_refused_are_counted_and_the_reason_is_shown() {
    // Findings long enough for three messages; the host takes the first, then refuses.
    let mut long = findings();
    long["mappings"] = json!(
        (0..40)
            .map(|n| json!({ "tracker": "alpha", "project": format!("P{n}"),
                "repository": format!("git@git.example.test:team/{}.git", "r".repeat(250)),
                "folders": [], "important": false }))
            .collect::<Vec<_>>()
    );
    let turns = vec![
        Turn::Calls(vec![
            write("findings.json", &long.to_string()),
            write(ACTIVITY_FILE, &dashboard(&[], &json!({})).to_string()),
        ]),
        Turn::Text("done"),
    ];
    let stand = Stand::new(turns);
    stand.rig.host.answer(200, STORED);
    stand.rig.host.answer(422, "{}");
    stand.ask("dashboard-setup-start");
    let done = stand.wait_for_state("done");
    assert_eq!(done["facts"]["stored"], 1, "{done}");
    assert!(
        done["facts"]["error"].as_str().unwrap().contains("422"),
        "{done}"
    );
    assert_eq!(stand.rig.host.count(), 2);
}

#[test]
fn a_host_that_is_not_running_does_not_undo_the_run() {
    let stand = Stand::new(run_of(&dashboard(&["lab"], &json!({}))));
    // Nothing listens on port 1.
    stand.rig.link.set(Connection {
        port: 1,
        token: "unused".into(),
    });
    stand.ask("dashboard-setup-start");
    let done = stand.wait_for_state("done");
    assert!(done["facts"]["error"].as_str().is_some(), "{done}");
    assert_eq!(done["facts"]["stored"], 0);
    let page: Value = serde_json::from_str(&stand.read(ACTIVITY_FILE).unwrap()).unwrap();
    assert_eq!(page["payload"]["hidden_projects"], json!(["lab"]));
}

#[test]
fn a_tool_call_that_is_still_writing_when_the_run_is_cancelled_is_waited_for() {
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let late = (Arc::clone(&started), Arc::clone(&release));
    let stand = Stand::build(
        vec![Turn::Calls(vec![("late", json!({}))]), Turn::Text("done")],
        |_| {},
        move |workspace| {
            Some(Arc::new(Late {
                workspace: Arc::clone(workspace),
                started: late.0,
                release: late.1,
            }))
        },
    );
    // Declared after the stand, so that it is dropped, and the tool let go, before the stand is.
    let _release = Release(Arc::clone(&release));
    let before = stand.read(ACTIVITY_FILE).unwrap();
    stand.ask("dashboard-setup-start");
    wait_for("the tool", || started.load(Ordering::SeqCst));
    stand.ask("dashboard-setup-cancel");
    // The tool is let go a moment after the stop; the old files are put back after its write.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(stand.status()["state"], "reading_and_building");
    release.store(true, Ordering::SeqCst);
    stand.wait_for_state("cancelled");
    assert_untouched(&stand, &before);
}

#[test]
fn a_run_that_is_cancelled_while_the_facts_are_sent_leaves_the_baseline_of_the_last_run() {
    let first = dashboard(&["lab"], &json!({ "shop": "PLAT" }));
    let second = dashboard(&[], &json!({ "shop": "Shop again", "blog": "The blog" }));
    let third = dashboard(&[], &json!({ "shop": "Shop three" }));
    let mut turns = run_of(&first);
    turns.extend(run_of(&second));
    turns.extend(run_of(&third));
    let stand = Stand::new(turns);
    stand.ask("dashboard-setup-start");
    stand.wait_for_state("done");
    let kept = baseline(&stand).unwrap();

    // The person hides the blog; the second run is cancelled while its facts are sent.
    let page = Dashboard::open(Arc::clone(&stand.rig.workspace));
    page.edit(&Edit::Hide("blog".into())).unwrap();
    let before = stand.read(ACTIVITY_FILE).unwrap();
    *stand.rig.host.delay.lock().unwrap() = Duration::from_millis(700);
    let calls = stand.rig.host.count();
    stand.ask("dashboard-setup-start");
    wait_for("the call to the host", || stand.rig.host.count() > calls);
    stand.ask("dashboard-setup-cancel");
    stand.wait_for_state("cancelled");
    assert_eq!(stand.read(ACTIVITY_FILE).as_deref(), Some(before.as_str()));
    assert_eq!(baseline(&stand).as_deref(), Some(kept.as_str()));

    // The next run still measures the person's changes from the last run that was done: the
    // blog stays hidden, and the session's own name for the shop is the session's to change.
    *stand.rig.host.delay.lock().unwrap() = Duration::ZERO;
    stand.ask("dashboard-setup-start");
    stand.wait_for_state("done");
    let page: Value = serde_json::from_str(&stand.read(ACTIVITY_FILE).unwrap()).unwrap();
    assert_eq!(page["payload"]["hidden_projects"], json!(["blog"]));
    assert_eq!(page["payload"]["project_names"]["shop"], "Shop three");
}

#[test]
fn a_widget_the_person_added_before_the_first_setup_is_kept() {
    let stand = Stand::new(run_of(&dashboard(&["lab"], &json!({}))));
    let mut document: Value = serde_json::from_str(&stand.read(ACTIVITY_FILE).unwrap()).unwrap();
    document["payload"]["widgets"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "id": "mine", "kind": "table", "group_by": "tool",
            "position": { "column": 0, "row": 14, "width": 12, "height": 3 } }));
    stand
        .rig
        .workspace
        .write(ACTIVITY_FILE, &document.to_string())
        .unwrap();
    stand.ask("dashboard-setup-start");
    stand.wait_for_state("done");
    let page: Value = serde_json::from_str(&stand.read(ACTIVITY_FILE).unwrap()).unwrap();
    let widgets = page["payload"]["widgets"].as_array().unwrap();
    assert!(widgets.iter().any(|w| w["id"] == "mine"), "{widgets:?}");
    assert_eq!(page["payload"]["hidden_projects"], json!(["lab"]));
}

#[test]
fn files_kept_aside_by_a_run_that_could_not_put_them_back_are_put_back_at_the_next_start() {
    let stand = Stand::with(Vec::new(), |data| {
        let aside = json!({ "dashboard": null, "findings": "{\"trackers\":[]}", "report": "old" });
        std::fs::write(data.join("setup-before.json"), aside.to_string()).unwrap();
        let state = json!({ "step": "failed", "error": "the old files could not all be put back" });
        std::fs::write(data.join(STATE_FILE), state.to_string()).unwrap();
    });
    assert_eq!(
        stand.read("findings.json").as_deref(),
        Some("{\"trackers\":[]}")
    );
    assert_eq!(stand.read("setup-report.md").as_deref(), Some("old"));
    assert!(!stand.rig.data.join("setup-before.json").exists());
}

#[test]
fn a_file_that_cannot_be_read_stops_the_run_before_it_starts() {
    let stand = Stand::with(Vec::new(), |data| {
        let dir = data.join("workspace/dashboards");
        std::fs::create_dir_all(&dir).unwrap();
        // Longer than the agent reads: it could not be put back as it was.
        std::fs::write(dir.join("activity.json"), "x".repeat(2 * 1024 * 1024)).unwrap();
    });
    stand.ask("dashboard-setup-start");
    let failed = stand.wait_for_state("failed");
    assert!(
        failed["error"]
            .as_str()
            .unwrap()
            .contains("could not be read"),
        "{failed}"
    );
    assert!(stand.rig.bodies().is_empty());
    assert_eq!(
        std::fs::metadata(stand.rig.data.join("workspace/dashboards/activity.json"))
            .unwrap()
            .len(),
        2 * 1024 * 1024
    );
}
