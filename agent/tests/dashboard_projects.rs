//! Hiding and renaming projects through the socket. Each test starts a socket server over a
//! temporary workspace, sends the commands the chat sends and looks at the answer, at the view
//! of a project table and at the file on disk.

#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use construct_agent::dashboard::{ACTIVITY_FILE, default_activity, ensure_default};
use construct_agent::dashboard_document::Dashboard;
use construct_agent::dashboard_projects::{Edit, view};
use construct_agent::ipc::{Server, request};
use construct_agent::workspace::Workspace;
use serde_json::{Value, json};

struct Stand {
    workspace: Arc<Workspace>,
    socket: PathBuf,
    root: PathBuf,
}

impl Stand {
    fn start(name: &str, text: Option<&str>) -> Self {
        let root = std::env::temp_dir().join(format!("cb-projects-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let workspace = Arc::new(Workspace::open(&root.join("data")).unwrap());
        match text {
            Some(text) => workspace.write(ACTIVITY_FILE, text).unwrap(),
            None => {
                ensure_default(&workspace).unwrap();
            }
        }
        let dashboard = Dashboard::open(Arc::clone(&workspace));
        let socket = root.join("run/construct/agent.sock");
        let server = Server::bind(&socket).unwrap().with_dashboard(dashboard);
        thread::spawn(move || server.serve());
        Self {
            workspace,
            socket,
            root,
        }
    }

    fn ask(&self, line: &str) -> Value {
        let reply = request(&self.socket, line, Duration::from_secs(5)).unwrap();
        serde_json::from_str(&reply).unwrap()
    }

    fn command(&self, command: &str, body: &Value) -> Value {
        self.ask(&format!("{command} {body}"))
    }

    /// The document as the file on disk has it.
    fn file(&self) -> Value {
        let text = self.workspace.read_whole(ACTIVITY_FILE, 1 << 20).unwrap();
        serde_json::from_str(&text).unwrap()
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn time() -> Vec<(String, u64)> {
    vec![
        ("/work/site".to_owned(), 60),
        ("/work/api".to_owned(), 30),
        ("/old/site".to_owned(), 10),
    ]
}

fn labels() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("/work/site".to_owned(), "work/site".to_owned()),
        ("/work/api".to_owned(), "api".to_owned()),
        ("/old/site".to_owned(), "old/site".to_owned()),
    ])
}

#[test]
fn hiding_a_project_takes_it_out_of_the_total_and_the_count_shows() {
    let stand = Stand::start("hide", None);
    let before = view(&time(), &labels(), &stand.ask("dashboard-get")["document"]);
    assert_eq!(before.total_parts, 100);
    assert_eq!(before.hidden_count, 0);

    let answer = stand.command("dashboard-hide", &json!({ "project": "/work/api" }));
    assert!(answer.get("error").is_none(), "{answer}");
    let after = view(&time(), &labels(), &answer["document"]);
    assert_eq!(after.total_parts, 70);
    assert_eq!(after.hidden_count, 1);
    assert!(after.rows.iter().all(|row| row.name != "api"));
    assert!((after.rows[0].share - 60.0 / 70.0).abs() < 1e-9);
    // The page asking again sees the same.
    assert_eq!(stand.ask("dashboard-get")["document"], answer["document"]);
}

#[test]
fn showing_a_project_brings_it_back() {
    let stand = Stand::start("show", None);
    stand.command("dashboard-hide", &json!({ "project": "/work/api" }));
    let answer = stand.command("dashboard-show", &json!({ "project": "/work/api" }));
    let shown = view(&time(), &labels(), &answer["document"]);
    assert_eq!(shown.total_parts, 100);
    assert_eq!(shown.hidden_count, 0);
    assert_eq!(stand.file()["payload"]["hidden_projects"], json!([]));
    // Hiding twice lists the project once.
    stand.command("dashboard-hide", &json!({ "project": "/work/api" }));
    stand.command("dashboard-hide", &json!({ "project": "/work/api" }));
    assert_eq!(
        stand.file()["payload"]["hidden_projects"],
        json!(["/work/api"])
    );
}

#[test]
fn renaming_to_an_existing_name_merges_two_rows() {
    let stand = Stand::start("merge", None);
    let before = view(&time(), &labels(), &stand.ask("dashboard-get")["document"]);
    assert_eq!(before.rows.len(), 3);

    let answer = stand.command(
        "dashboard-rename",
        &json!({ "project": "/old/site", "name": "work/site" }),
    );
    assert!(answer.get("error").is_none(), "{answer}");
    let after = view(&time(), &labels(), &answer["document"]);
    assert_eq!(after.rows.len(), 2);
    assert_eq!(after.rows[0].name, "work/site");
    assert_eq!(after.rows[0].parts, 70);
    assert_eq!(after.total_parts, 100);
}

#[test]
fn each_action_is_in_the_file_on_disk() {
    let stand = Stand::start("disk", None);
    stand.command("dashboard-hide", &json!({ "project": "/work/api" }));
    assert_eq!(
        stand.file()["payload"]["hidden_projects"],
        json!(["/work/api"])
    );
    stand.command(
        "dashboard-rename",
        &json!({ "project": "/work/site", "name": "Website" }),
    );
    assert_eq!(
        stand.file()["payload"]["project_names"],
        json!({ "/work/site": "Website" })
    );
    stand.command("dashboard-show", &json!({ "project": "/work/api" }));
    assert_eq!(stand.file()["payload"]["hidden_projects"], json!([]));
}

#[test]
fn two_commands_in_a_row_both_stay() {
    let stand = Stand::start("row", None);
    stand.command("dashboard-hide", &json!({ "project": "/work/api" }));
    let answer = stand.command(
        "dashboard-rename",
        &json!({ "project": "/work/site", "name": "Website" }),
    );
    for document in [answer["document"].clone(), stand.file()] {
        assert_eq!(document["payload"]["hidden_projects"], json!(["/work/api"]));
        assert_eq!(
            document["payload"]["project_names"],
            json!({ "/work/site": "Website" })
        );
    }
}

#[test]
fn a_change_made_to_the_file_by_hand_stays_through_an_edit() {
    let mut document = default_activity();
    document["title"] = json!("My week");
    document["payload"]["period"] = json!("last_month");
    document["payload"]["widgets"].as_array_mut().unwrap().pop();
    let stand = Stand::start("fields", Some(&document.to_string()));
    stand.command("dashboard-hide", &json!({ "project": "/work/api" }));
    let mut expected = document;
    expected["payload"]["hidden_projects"] = json!(["/work/api"]);
    assert_eq!(stand.file(), expected);
}

#[test]
fn commands_at_the_same_time_all_stay() {
    let stand = Arc::new(Stand::start("parallel", None));
    let threads: Vec<_> = (0..8)
        .map(|thread| {
            let stand = Arc::clone(&stand);
            thread::spawn(move || {
                for n in 0..10 {
                    let answer = stand.command(
                        "dashboard-hide",
                        &json!({ "project": format!("/work/p-{thread}-{n}") }),
                    );
                    assert!(answer.get("error").is_none(), "{answer}");
                }
            })
        })
        .collect();
    for handle in threads {
        handle.join().unwrap();
    }
    let hidden = stand.file()["payload"]["hidden_projects"].clone();
    assert_eq!(hidden.as_array().unwrap().len(), 80);
}

#[test]
fn an_edit_starts_from_the_file_as_it_is_and_not_from_the_last_one_read() {
    let stand = Stand::start("fresh", None);
    // Written after the agent read the file, and not read again by the time the command comes.
    let mut document = default_activity();
    document["title"] = json!("Changed by the chat");
    stand
        .workspace
        .write(ACTIVITY_FILE, &document.to_string())
        .unwrap();
    stand.command("dashboard-hide", &json!({ "project": "/work/api" }));
    assert_eq!(stand.file()["title"], "Changed by the chat");
    assert_eq!(
        stand.file()["payload"]["hidden_projects"],
        json!(["/work/api"])
    );
}

#[test]
fn an_edit_that_changes_nothing_leaves_the_file_as_it_is() {
    let text = format!("{}\n", serde_json::to_string(&default_activity()).unwrap());
    let stand = Stand::start("nothing", Some(&text));
    stand.command("dashboard-show", &json!({ "project": "/work/api" }));
    stand.command("dashboard-hide", &json!({ "project": "/work/api" }));
    let hidden = stand.workspace.read_whole(ACTIVITY_FILE, 1 << 20).unwrap();
    stand.command("dashboard-hide", &json!({ "project": "/work/api" }));
    assert_eq!(
        stand.workspace.read_whole(ACTIVITY_FILE, 1 << 20).unwrap(),
        hidden
    );
    stand.command(
        "dashboard-rename",
        &json!({ "project": "/work/site", "name": "Website" }),
    );
    let renamed = stand.workspace.read_whole(ACTIVITY_FILE, 1 << 20).unwrap();
    stand.command(
        "dashboard-rename",
        &json!({ "project": "/work/site", "name": "Website" }),
    );
    assert_eq!(
        stand.workspace.read_whole(ACTIVITY_FILE, 1 << 20).unwrap(),
        renamed
    );
    // Showing a project that is not hidden does not even rewrite the first text.
    let again = Stand::start("nothing-show", Some(&text));
    again.command("dashboard-show", &json!({ "project": "/work/api" }));
    assert_eq!(
        again.workspace.read_whole(ACTIVITY_FILE, 1 << 20).unwrap(),
        text
    );
}

#[test]
fn a_result_the_reader_would_refuse_is_not_written() {
    let root = std::env::temp_dir().join(format!("cb-projects-long-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let workspace = Arc::new(Workspace::open(&root).unwrap());
    ensure_default(&workspace).unwrap();
    let dashboard = Dashboard::open(Arc::clone(&workspace));
    let before = workspace.read_whole(ACTIVITY_FILE, 1 << 20).unwrap();
    let error = dashboard
        .edit(&Edit::Rename {
            project: "/work/site".to_owned(),
            name: "x".repeat(1 << 20),
        })
        .unwrap_err();
    assert!(error.contains("too long"), "{error}");
    assert_eq!(
        workspace.read_whole(ACTIVITY_FILE, 1 << 20).unwrap(),
        before
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_bad_request_is_a_plain_text_and_the_file_is_not_touched() {
    let stand = Stand::start("bad", None);
    let before = stand.file();
    for line in [
        "dashboard-hide",
        "dashboard-hide not json",
        r#"dashboard-hide {"project": ""}"#,
        r#"dashboard-hide {"project": "/a", "name": "x"}"#,
        r#"dashboard-rename {"project": "/a"}"#,
        r#"dashboard-rename {"project": "/a", "name": 3}"#,
    ] {
        let answer = stand.ask(line);
        assert!(answer["error"].is_string(), "{line}: {answer}");
        assert!(answer.get("document").is_none());
    }
    assert_eq!(stand.file(), before);
}

#[test]
fn a_file_that_does_not_pass_is_not_edited() {
    let stand = Stand::start("invalid", Some("{ not json"));
    let answer = stand.command("dashboard-hide", &json!({ "project": "/work/api" }));
    assert!(answer["error"].as_str().unwrap().contains("not changed"));
    assert_eq!(
        stand.workspace.read_whole(ACTIVITY_FILE, 1 << 20).unwrap(),
        "{ not json"
    );
}
