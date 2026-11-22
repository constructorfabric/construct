//! The setup session through the whole chat loop: a real chat with the setup tools over a real
//! workspace folder holding fixture session files (two trackers, three repositories), answering a
//! scripted fake provider.
#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use construct_agent::chat::{Chat, Usage};

use construct_agent::chat::setup::{CONVERSATION, INSTRUCTION, Outcome};
use construct_agent::chat::tool::{ChatTool, ChatTools, setup_tools, standard_tools};
use construct_agent::dashboard::{
    ACTIVITY_FILE, default_activity, ensure_default, validate_activity,
};
use construct_agent::records::FileSink;
use construct_agent::settings::Settings;
use construct_agent::workspace::{DIR_NAME, Workspace};
use serde_json::{Value, json};

mod common;

use common::bodies::{calls, names, results};
use common::scripted::{Turn, provider};
use common::sessions::render_tool;
use common::stand::{deadlines, use_provider, wait_for};

static NEXT: AtomicU32 = AtomicU32::new(0);

const REPORT: &str = "# Setup report\n\nFound two trackers and three repositories.\n\nNot sure: the blog repository has no ticket in any session.\n";

/// What the model reads: three sessions, two trackers (PLAT at alpha, BLOG at beta), and one
/// folder for each of the three repositories.
const SESSIONS: [(&str, &str); 3] = [
    (
        "history/0001-claude-code-shop.txt",
        "user: fix the checkout, ticket PLAT-12 on https://alpha.example.test/browse/PLAT-12\n",
    ),
    (
        "history/0002-codex-blog.txt",
        "user: write the post, ticket BLOG-3 on https://beta.example.test/issues/BLOG-3\n",
    ),
    (
        "history/0003-claude-code-lab.txt",
        "user: try the parser in the lab, no ticket\n",
    ),
];

struct Stand {
    base: PathBuf,
    data: PathBuf,
    workspace: PathBuf,
    chat: Chat,
    bodies: Arc<Mutex<Vec<String>>>,
    usage: Arc<Mutex<Vec<Usage>>>,
    _rt: tokio::runtime::Runtime,
}

/// What a stand is built with.
#[derive(Clone, Copy)]
struct Setup {
    model: bool,
    tools: bool,
}

const FULL: Setup = Setup {
    model: true,
    tools: true,
};

impl Stand {
    fn new(turns: Vec<Turn>) -> Self {
        Self::with(|_| turns, FULL, None)
    }

    /// A stand whose script is made from the folder of its first repository.
    fn scripted(turns: impl FnOnce(&Path) -> Vec<Turn>) -> Self {
        Self::with(|base| turns(&base.join("repos").join("shop")), FULL, None)
    }

    fn with(
        turns: impl FnOnce(&Path) -> Vec<Turn>,
        setup: Setup,
        extra: Option<Arc<dyn ChatTool>>,
    ) -> Self {
        let base = std::env::temp_dir().join(format!(
            "cb-setup-session-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let data = base.join("data");
        let (addr, bodies) = provider(turns(&base));
        let settings = Settings::open(&base.join("config")).unwrap();
        if setup.model {
            use_provider(&settings, addr);
        }
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let workspace = Arc::new(Workspace::open(&data).unwrap());
        ensure_default(&workspace).unwrap();
        let render = render_tool(&workspace);
        let usage = Arc::new(Mutex::new(Vec::new()));
        let reported = Arc::clone(&usage);
        let mut chat = Chat::new(settings, &data, rt.handle().clone(), deadlines())
            .unwrap()
            .with_usage(move |u| reported.lock().unwrap().push(u))
            .with_tools(ChatTools::new(standard_tools(&workspace, Some(render.clone()))).unwrap());
        if setup.tools {
            let mut tools = setup_tools(&workspace, Some(render));
            // Before the sub-agent tool, which is always last.
            tools.splice(tools.len() - 1..tools.len() - 1, extra);
            chat = chat.with_setup_tools(ChatTools::new(tools).unwrap());
        }
        // The session files, as the run before this one leaves them.
        let repos: Vec<PathBuf> = ["shop", "blog", "lab"]
            .iter()
            .map(|name| {
                let repo = base.join("repos").join(name);
                std::fs::create_dir_all(repo.join(".git")).unwrap();
                std::fs::write(
                    repo.join(".git/config"),
                    format!("[remote \"origin\"]\n\turl = git@git.example.test:team/{name}.git\n"),
                )
                .unwrap();
                std::fs::write(repo.join("README.md"), format!("The {name} project.\n")).unwrap();
                repo
            })
            .collect();
        let folders: Vec<Value> = repos
            .iter()
            .map(|repo| {
                let name = repo.file_name().unwrap().to_str().unwrap();
                json!({ "folder": repo, "remotes": [
                    { "name": "origin", "url": format!("git@git.example.test:team/{name}.git") }] })
            })
            .collect();
        let dir = data.join(DIR_NAME);
        std::fs::create_dir_all(dir.join("history")).unwrap();
        for (path, text) in SESSIONS {
            std::fs::write(dir.join(path), text).unwrap();
        }
        std::fs::write(
            dir.join("history/folders.json"),
            json!({ "folders": folders }).to_string(),
        )
        .unwrap();
        Self {
            workspace: dir,
            data,
            base,
            chat,
            bodies,
            usage,
            _rt: rt,
        }
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.workspace.join(path)).unwrap()
    }

    fn bodies(&self) -> Vec<Value> {
        self.bodies
            .lock()
            .unwrap()
            .iter()
            .map(|body| serde_json::from_str(body).unwrap())
            .collect()
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn findings() -> Value {
    json!({
        "trackers": [
            { "name": "alpha", "kind": "issue tracker", "evidence": "PLAT-12 on alpha.example.test" },
            { "name": "beta", "kind": "issue tracker", "evidence": "BLOG-3 on beta.example.test" },
        ],
        "mappings": [
            { "tracker": "alpha", "project": "PLAT", "repository": "git@git.example.test:team/shop.git",
              "folders": ["shop"], "important": true },
            { "tracker": "beta", "project": "BLOG", "repository": "git@git.example.test:team/blog.git",
              "folders": ["blog"], "important": true },
            { "tracker": "", "project": "", "repository": "git@git.example.test:team/lab.git",
              "folders": ["lab"], "important": false },
        ],
        "groupings": [{ "name": "work", "projects": ["PLAT", "BLOG"] }],
    })
}

/// The page for that work: the default widgets, the lab hidden, the shop and blog named.
fn dashboard() -> Value {
    let mut document = default_activity();
    document["payload"]["hidden_projects"] = json!(["lab"]);
    document["payload"]["project_names"] = json!({ "shop": "PLAT", "blog": "BLOG" });
    document
}

fn write(path: &str, content: &str) -> (&'static str, Value) {
    ("Write", json!({ "file_path": path, "content": content }))
}

/// The three files, as one turn of calls.
fn write_all(dashboard: &Value) -> Turn {
    Turn::Calls(vec![
        write("findings.json", &findings().to_string()),
        write("setup-report.md", REPORT),
        write(ACTIVITY_FILE, &dashboard.to_string()),
    ])
}

/// The script of a run that reads the files itself.
fn reading_itself(dashboard: &Value, repo: &Path) -> Vec<Turn> {
    vec![
        Turn::Calls(vec![
            ("Glob", json!({ "pattern": "history/*" })),
            (
                "Read",
                json!({ "file_path": "history/0001-claude-code-shop.txt" }),
            ),
            (
                "Read",
                json!({ "file_path": "history/0002-codex-blog.txt" }),
            ),
            (
                "Read",
                json!({ "file_path": "history/0003-claude-code-lab.txt" }),
            ),
            ("Read", json!({ "file_path": "history/folders.json" })),
            ("list_folder", json!({ "path": repo })),
            ("Read", json!({ "file_path": ACTIVITY_FILE })),
        ]),
        write_all(dashboard),
        Turn::Calls(vec![("render_dashboard", json!({}))]),
        Turn::Text("The dashboard is set up."),
    ]
}

#[test]
fn the_session_reads_the_files_and_writes_the_findings_the_report_and_the_dashboard() {
    let stand = Stand::scripted(|repo| reading_itself(&dashboard(), repo));
    assert_eq!(stand.chat.setup(), Ok(Outcome::Done));

    let findings = serde_json::from_str::<Value>(&stand.read("findings.json")).unwrap();
    assert_eq!(findings["trackers"].as_array().unwrap().len(), 2);
    assert_eq!(findings["mappings"].as_array().unwrap().len(), 3);
    assert_eq!(stand.read("setup-report.md"), REPORT);
    let document = serde_json::from_str::<Value>(&stand.read(ACTIVITY_FILE)).unwrap();
    assert_eq!(validate_activity(&document), Ok(()));
    assert_eq!(document["payload"]["hidden_projects"], json!(["lab"]));

    // The bytes of the run are reported once, as an answer's are.
    let usage = stand.usage.lock().unwrap();
    assert_eq!(usage.len(), 1);
    assert!(usage[0].sent > 0 && usage[0].received > 0, "{usage:?}");
    drop(usage);

    let bodies = stand.bodies();
    assert_eq!(bodies.len(), 4);
    // The model saw the files and the repository, and the page after it was written.
    let first = results(&bodies[1]);
    assert!(first[1].contains("PLAT-12"), "{first:?}");
    assert!(first[2].contains("BLOG-3"), "{first:?}");
    assert!(
        first[4].contains("git@git.example.test:team/blog.git"),
        "{first:?}"
    );
    assert!(first[5].contains("README.md"), "{first:?}");
    let last = results(&bodies[3]);
    assert_eq!(calls(&bodies[3]).last().unwrap(), "render_dashboard");
    let seen = last.last().unwrap();
    assert!(seen.starts_with("Dashboard \"Activity\""), "{seen}");
    assert!(!seen.contains("error"), "{seen}");
}

#[test]
fn a_document_the_tool_reports_as_broken_is_repaired() {
    let mut broken = dashboard();
    broken["payload"]["widgets"][2]
        .as_object_mut()
        .unwrap()
        .remove("group_by");
    let stand = Stand::new(vec![
        Turn::Calls(vec![write(ACTIVITY_FILE, &broken.to_string())]),
        Turn::Calls(vec![("render_dashboard", json!({}))]),
        Turn::Calls(vec![write(ACTIVITY_FILE, &dashboard().to_string())]),
        Turn::Calls(vec![("render_dashboard", json!({}))]),
        Turn::Text("Fixed."),
    ]);
    assert_eq!(stand.chat.setup(), Ok(Outcome::Done));

    let bodies = stand.bodies();
    let reported = results(&bodies[2]);
    assert!(
        reported[1].starts_with(
            "The dashboard file has an error. The page keeps showing the last good version.\n- /payload/widgets/2/group_by: "
        ),
        "{reported:?}"
    );
    let repaired = results(&bodies[4]);
    assert!(
        repaired[3].starts_with("Dashboard \"Activity\""),
        "{repaired:?}"
    );
    let document = serde_json::from_str::<Value>(&stand.read(ACTIVITY_FILE)).unwrap();
    assert_eq!(validate_activity(&document), Ok(()));
}

/// The files a finished run leaves, as text.
fn written(stand: &Stand) -> [String; 3] {
    [
        stand.read("findings.json"),
        stand.read("setup-report.md"),
        stand.read(ACTIVITY_FILE),
    ]
}

#[test]
fn a_run_that_hands_the_files_to_a_sub_agent_ends_as_one_that_reads_them_itself() {
    let alone = Stand::scripted(|repo| reading_itself(&dashboard(), repo));
    assert_eq!(alone.chat.setup(), Ok(Outcome::Done));

    let helped = Stand::new(vec![
        Turn::Calls(vec![(
            "run_subagent",
            json!({ "task": "Read the sessions and name each tracker and ticket in them.",
                    "files": ["history/0001-claude-code-shop.txt", "history/0002-codex-blog.txt"] }),
        )]),
        // The sub-agent's own turns.
        Turn::Calls(vec![(
            "Read",
            json!({ "file_path": "history/0001-claude-code-shop.txt" }),
        )]),
        Turn::Text("PLAT-12 on alpha; BLOG-3 on beta."),
        write_all(&dashboard()),
        Turn::Calls(vec![("render_dashboard", json!({}))]),
        Turn::Text("The dashboard is set up."),
    ]);
    assert_eq!(helped.chat.setup(), Ok(Outcome::Done));

    assert_eq!(written(&helped), written(&alone));
    let bodies = helped.bodies();
    // The helper starts from the task alone, with its own instruction and no sub-agent tool.
    let sub = &bodies[1];
    assert_ne!(sub["messages"][0], bodies[0]["messages"][0]);
    assert_eq!(sub["messages"].as_array().unwrap().len(), 2);
    assert!(sub["messages"][1]["content"].as_str().unwrap().contains(
        "Files to start with: history/0001-claude-code-shop.txt, history/0002-codex-blog.txt"
    ));
    let told = sub["messages"][0]["content"].as_str().unwrap();
    assert!(told.contains("only to read"), "{told}");
    assert!(told.contains("never a file in `history`"), "{told}");
    // It read the file itself, with the tools it was given.
    assert!(results(&bodies[2])[0].contains("PLAT-12"));
    assert!(!names(sub).contains(&"run_subagent".to_string()));
    assert!(names(sub).contains(&"read_file".to_string()));
    // Only its answer comes back to the run, which does not hold the helper's reading.
    let back = results(&bodies[3]);
    assert_eq!(back, ["PLAT-12 on alpha; BLOG-3 on beta."]);
    assert!(!bodies[3].to_string().contains("ticket PLAT-12 on https"));
}

#[test]
fn the_conversation_is_not_in_the_chat_list_and_is_stored_nowhere() {
    let stand = Stand::scripted(|repo| reading_itself(&dashboard(), repo));
    assert_eq!(stand.chat.setup(), Ok(Outcome::Done));

    assert_eq!(
        stand.chat.list(),
        json!({ "conversations": [] }).to_string()
    );
    let sink = FileSink::new(&stand.data);
    for source in ["chat", CONVERSATION] {
        assert!(sink.list_sessions(source).unwrap().is_empty(), "{source}");
    }
    // No conversation of the chat can have that name either.
    assert_eq!(
        stand.chat.get(CONVERSATION),
        [json!({ "error": "unknown conversation" }).to_string()]
    );
}

#[test]
fn a_stop_ends_the_run_and_keeps_what_was_written() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![
            write("findings.json", &findings().to_string()),
            write("setup-report.md", REPORT),
        ]),
        Turn::Silence,
    ]);
    let chat = stand.chat.clone();
    let run = thread::spawn(move || chat.setup());
    wait_for("the second request", || {
        stand.bodies.lock().unwrap().len() == 2
    });
    assert!(stand.chat.stop(CONVERSATION));
    assert_eq!(run.join().unwrap(), Ok(Outcome::Stopped));

    assert_eq!(stand.read("setup-report.md"), REPORT);
    assert!(stand.read("findings.json").contains("alpha"));
    // The slot is free again.
    assert!(!stand.chat.stop(CONVERSATION));
}

#[test]
fn a_chat_answer_and_a_setup_run_at_once_neither_is_refused() {
    // The setup's request is the first and gets no answer; the chat's is the second.
    let stand = Stand::new(vec![Turn::Silence, Turn::Text("chat reply")]);
    let chat = stand.chat.clone();
    let run = thread::spawn(move || chat.setup());
    wait_for("the setup's request", || {
        stand.bodies.lock().unwrap().len() == 1
    });

    let mut lines = Vec::<String>::new();
    stand.chat.send(
        &json!({ "message": "a question beside the setup" }).to_string(),
        &mut |line| {
            lines.push(line.to_owned());
            true
        },
    );
    assert!(lines.iter().any(|l| l.contains("chat reply")), "{lines:?}");
    assert_eq!(lines.last().unwrap(), r#"{"event":"done"}"#, "{lines:?}");

    // The setup run is still going, and still stops by its own name.
    assert!(!run.is_finished());
    assert!(stand.chat.stop(CONVERSATION));
    assert_eq!(run.join().unwrap(), Ok(Outcome::Stopped));
}

#[test]
fn a_stop_ends_a_run_that_waits_for_a_helper() {
    let gate = Arc::new(AtomicBool::new(false));
    let stand = Stand::new(vec![
        Turn::Calls(vec![(
            "run_subagent",
            json!({ "task": "Read the sessions." }),
        )]),
        Turn::Gated(Arc::clone(&gate), Box::new(Turn::Text("late"))),
    ]);
    let chat = stand.chat.clone();
    let run = thread::spawn(move || chat.setup());
    wait_for("the helper's request", || {
        stand.bodies.lock().unwrap().len() == 2
    });
    assert!(stand.chat.stop(CONVERSATION));
    assert_eq!(run.join().unwrap(), Ok(Outcome::Stopped));
    gate.store(true, Ordering::SeqCst);
}

#[test]
fn with_no_model_the_run_refuses_with_a_plain_reason() {
    let stand = Stand::with(
        |_| vec![Turn::Text("never asked")],
        Setup {
            model: false,
            tools: true,
        },
        None,
    );
    assert_eq!(stand.chat.setup(), Err("no model".to_string()));
    assert!(stand.bodies().is_empty());
    assert!(!stand.workspace.join("findings.json").exists());
}

#[test]
fn with_no_tools_the_run_refuses_with_a_plain_reason() {
    let stand = Stand::with(
        |_| vec![Turn::Text("never asked")],
        Setup {
            model: true,
            tools: false,
        },
        None,
    );
    assert_eq!(stand.chat.setup(), Err("no tools".to_string()));
    assert!(stand.bodies().is_empty());
}

#[test]
fn a_failed_answer_is_a_plain_reason_and_keeps_what_was_written() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![write("setup-report.md", REPORT)]),
        Turn::Error,
    ]);
    assert_eq!(stand.chat.setup(), Err("error 500".to_string()));
    assert_eq!(stand.read("setup-report.md"), REPORT);
}

#[test]
fn the_instruction_and_the_tool_list_are_the_same_in_every_request() {
    let stand = Stand::new(vec![
        Turn::Calls(vec![(
            "Read",
            json!({ "file_path": "history/folders.json" }),
        )]),
        Turn::Text("Nothing to set up."),
    ]);
    assert_eq!(stand.chat.setup(), Ok(Outcome::Done));

    let bodies = stand.bodies();
    assert_eq!(bodies.len(), 2);
    for body in &bodies {
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], INSTRUCTION);
        assert_eq!(body["tools"], bodies[0]["tools"]);
    }
    assert_eq!(
        names(&bodies[0]),
        [
            "Read",
            "Write",
            "Edit",
            "Glob",
            "bash",
            "read_file",
            "list_folder",
            "find_files",
            "search_text",
            "render_dashboard",
            "run_subagent",
        ]
    );
    // What it must say, and what it must leave to the model.
    for said in [
        "never invent a tracker",
        "say what you are unsure of",
        "You must only read",
        "go back to the original file whenever a detail matters",
        "findings.json",
        "setup-report.md",
        ACTIVITY_FILE,
        "render_dashboard",
    ] {
        assert!(
            INSTRUCTION.to_lowercase().contains(&said.to_lowercase()),
            "{said}"
        );
    }
    for step in ["step 1", "step by step", "then read", "in this order"] {
        assert!(!INSTRUCTION.to_lowercase().contains(step), "{step}");
    }
}

#[test]
fn the_session_cannot_write_the_folder_list_or_any_session_file() {
    let list = "history/folders.json";
    let stand = Stand::new(vec![
        Turn::Calls(vec![
            write(list, r#"{"folders":[{"folder":"/","remotes":[]}]}"#),
            (
                "Edit",
                json!({ "file_path": list, "old_string": "folders", "new_string": "x" }),
            ),
            write("./History/folders.json", "{}"),
            write("dashboards/../history/0001-claude-code-shop.txt", "changed"),
            // These are not in the folder: the guard is not a ban on the word.
            write("history-notes.txt", "kept"),
            write("notes/history/a.txt", "kept"),
        ]),
        Turn::Text("done"),
    ]);
    let folders = stand.read(list);
    let session = stand.read(SESSIONS[0].0);
    assert_eq!(stand.chat.setup(), Ok(Outcome::Done));

    let told = results(&stand.bodies()[1]);
    for refusal in &told[..4] {
        assert_eq!(
            refusal,
            "Error: the folder history is only for reading in this session"
        );
    }
    assert_eq!(stand.read(list), folders);
    assert_eq!(stand.read(SESSIONS[0].0), session);
    assert_eq!(stand.read("history-notes.txt"), "kept");
    assert_eq!(stand.read("notes/history/a.txt"), "kept");
}

/// A tool that does not return until it is let go, as a slow command does.
struct Hang {
    started: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
}

impl ChatTool for Hang {
    fn name(&self) -> &'static str {
        "hang"
    }

    fn description(&self) -> &'static str {
        "Waits."
    }

    fn schema(&self) -> &'static str {
        r#"{"type":"object","properties":{}}"#
    }

    fn run(&self, _: &str) -> Result<String, String> {
        self.started.store(true, Ordering::SeqCst);
        while !self.release.load(Ordering::SeqCst) {
            thread::sleep(std::time::Duration::from_millis(10));
        }
        Ok("late".into())
    }
}

/// Lets the tool go when the test ends, however it ends: a failed wait must not leave the run
/// blocked on it, or the runtime would wait for it at shutdown and the failure would be a hang.
struct Release(Arc<AtomicBool>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[test]
fn a_stop_ends_the_run_while_a_tool_is_still_running() {
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let hang = Hang {
        started: Arc::clone(&started),
        release: Arc::clone(&release),
    };
    let stand = Stand::with(
        |_| vec![Turn::Calls(vec![("hang", json!({}))])],
        FULL,
        Some(Arc::new(hang)),
    );
    // After the stand, so that it drops first: the stand's runtime waits for the tool.
    let _release = Release(Arc::clone(&release));
    let chat = stand.chat.clone();
    let run = thread::spawn(move || chat.setup());
    wait_for("the tool to start", || started.load(Ordering::SeqCst));
    assert!(stand.chat.stop(CONVERSATION));
    // The run ends while the tool is still blocked: it does not wait for it.
    wait_for("the run to end", || run.is_finished());
    assert!(!release.load(Ordering::SeqCst));
    assert_eq!(run.join().unwrap(), Ok(Outcome::Stopped));
}
