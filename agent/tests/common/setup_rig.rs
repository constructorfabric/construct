//! What the tests of the dashboard setup job share: the made-up histories of two trackers and
//! three repositories, the scripted model's files, and a rig of a chat, a fake host of Construct
//! and a job over one data directory.
#![allow(clippy::unwrap_used, dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use connectors::{Author, Record, Role, Turn as Prompt};
use construct_agent::activity::Feed;
use construct_agent::chat::tool::{ChatTool, ChatTools, setup_tools, standard_tools};
use construct_agent::chat::{Chat, Link};
use construct_agent::dashboard::{ACTIVITY_FILE, default_activity, ensure_default};
use construct_agent::dashboard_setup::SetupJob;
use construct_agent::manage::Manager;
use construct_agent::records::{FileSink, Sink};
use construct_agent::settings::Settings;
use construct_agent::workspace::Workspace;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::fake_host::FakeHost;
use super::hold::Hold;
use super::scripted::{Turn, provider};
use super::sessions::render_tool;
use super::stand::{deadlines, timing, use_provider};

/// A sentence in the made-up sessions that no answer of the job may repeat.
pub const SECRET: &str = "the quarterly figures are in the green folder";
pub const REPORT: &str =
    "# Setup report\n\nTwo trackers, alpha and beta, and three repositories.\n";

/// A time a number of minutes ago.
pub fn at(minutes_ago: i64) -> String {
    (OffsetDateTime::now_utc() - time::Duration::minutes(minutes_ago))
        .format(&Rfc3339)
        .unwrap()
}

/// A whole minute in UTC, as a day starts and ends.
pub fn minute(minutes_ago: i64) -> String {
    (OffsetDateTime::now_utc() - time::Duration::minutes(minutes_ago))
        .replace_second(0)
        .unwrap()
        .replace_nanosecond(0)
        .unwrap()
        .format(&Rfc3339)
        .unwrap()
}

/// The three repositories: where they are, and the remote each has.
pub fn repositories(root: &Path) -> Vec<(&'static str, PathBuf)> {
    ["shop", "blog", "lab"]
        .into_iter()
        .map(|name| {
            let repo = root.join("repos").join(name);
            std::fs::create_dir_all(repo.join(".git")).unwrap();
            std::fs::write(
                repo.join(".git/config"),
                format!("[remote \"origin\"]\n\turl = git@git.example.test:team/{name}.git\n"),
            )
            .unwrap();
            (name, repo)
        })
        .collect()
}

/// One session in each repository, in the record files. The shop's branch names a ticket of the
/// other tracker, and the blog's session names the shop's ticket more often than its own: only the
/// tracker of the folder tells which is right.
pub fn write_histories(data: &Path, repos: &[(&'static str, PathBuf)]) {
    let mut sink = FileSink::new(data);
    let sessions = [
        (
            "claude-code",
            "shopsess-1",
            "shop",
            "feature/BLOG-3-copy",
            "PLAT-12",
            180,
        ),
        (
            "codex",
            "blogsess-1",
            "blog",
            "BLOG-3-post",
            "PLAT-12 PLAT-12",
            120,
        ),
        ("claude-code", "labsess-01", "lab", "main", "nothing", 60),
    ];
    for (tool, session, name, branch, named, minutes_ago) in sessions {
        let folder = repos.iter().find(|(n, _)| *n == name).unwrap().1.clone();
        for (n, role) in [(0, Role::User), (1, Role::Assistant), (2, Role::User)] {
            sink.append(&Record::Turn(Prompt {
                tool: tool.into(),
                session: session.into(),
                time: at(minutes_ago - n),
                folder: folder.to_string_lossy().into_owned(),
                branch: branch.into(),
                role,
                author: Author::Person,
                text: format!("work on the {name}, about {named}: {SECRET}"),
                tools: Vec::new(),
            }))
            .unwrap();
        }
    }
}

/// A call of the model's `Write` tool.
pub fn write(path: &str, content: &str) -> (&'static str, Value) {
    ("Write", json!({ "file_path": path, "content": content }))
}

/// What the model found in those histories.
pub fn findings() -> Value {
    json!({
        "trackers": [
            { "name": "alpha", "kind": "issue tracker", "evidence": "PLAT-12 in the shop sessions" },
            { "name": "beta", "kind": "issue tracker", "evidence": "BLOG-3 in the blog sessions" },
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

/// A dashboard document with these hidden projects and names.
pub fn dashboard(hidden: &[&str], names: &Value) -> Value {
    let mut document = default_activity();
    document["payload"]["hidden_projects"] = json!(hidden);
    document["payload"]["project_names"] = names.clone();
    document
}

/// A run's model turns: it writes the three files, then answers.
pub fn run_of(document: &Value) -> Vec<Turn> {
    vec![
        Turn::Calls(vec![
            write("findings.json", &findings().to_string()),
            write("setup-report.md", REPORT),
            write(ACTIVITY_FILE, &document.to_string()),
        ]),
        Turn::Text("The dashboard is set up."),
    ]
}

/// A job over these parts, with the facts of a manager that sends to `link` and a hook that holds
/// a run where `hold` says.
// One argument for each part the rig shares between its jobs; a struct would only move the list.
#[allow(clippy::too_many_arguments)]
fn job_over(
    settings: &Settings,
    chat: &Chat,
    link: &Link,
    workspace: &Arc<Workspace>,
    data: &Path,
    hold: &Arc<Hold>,
    feed: Option<Feed>,
    tool_wait: Option<std::time::Duration>,
) -> SetupJob {
    let manager = Manager::open_with(data, settings.clone(), link.clone(), timing()).unwrap();
    let gate = Arc::clone(hold);
    let job = SetupJob::open(settings.clone(), chat.clone(), Arc::clone(workspace), data)
        .with_facts(manager)
        .with_step_hook(move |step| gate.pass(step));
    let job = match tool_wait {
        Some(wait) => job.with_tool_wait(wait),
        None => job,
    };
    match feed {
        Some(feed) => job.with_feed(feed),
        None => job,
    }
}

/// A chat, a fake host and a job over one data directory.
pub struct Rig {
    pub data: PathBuf,
    pub settings: Settings,
    pub workspace: Arc<Workspace>,
    pub chat: Chat,
    pub link: Link,
    pub host: FakeHost,
    pub hold: Arc<Hold>,
    pub bodies: Arc<Mutex<Vec<String>>>,
    pub repos: Vec<(&'static str, PathBuf)>,
    pub job: SetupJob,
}

impl Rig {
    /// The rig in `base`, whose model answers from `turns`. `before` may change the data
    /// directory before the job is opened; a `feed` is told to read the findings; `extra` may
    /// give the session one more tool, made over the workspace; `tool_wait` is how long a run waits
    /// for a tool call that is still running when the session has ended.
    pub fn new(
        base: &Path,
        handle: tokio::runtime::Handle,
        turns: Vec<Turn>,
        feed: Option<Feed>,
        before: impl FnOnce(&Path),
        extra: impl FnOnce(&Arc<Workspace>) -> Option<Arc<dyn ChatTool>>,
        tool_wait: Option<std::time::Duration>,
    ) -> Self {
        let data = base.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let repos = repositories(base);
        write_histories(&data, &repos);
        before(&data);
        let (addr, bodies) = provider(turns);
        let settings = Settings::open(&base.join("config")).unwrap();
        use_provider(&settings, addr);
        let workspace = Arc::new(Workspace::open(&data).unwrap());
        ensure_default(&workspace).unwrap();
        let link = Link::default();
        let host = FakeHost::start();
        link.set(host.connection());
        let chat = Chat::new(settings.clone(), &data, handle, deadlines())
            .unwrap()
            .with_link(link.clone())
            .with_tools(
                ChatTools::new(standard_tools(&workspace, Some(render_tool(&workspace)))).unwrap(),
            )
            .with_setup_tools({
                let mut tools = setup_tools(&workspace, Some(render_tool(&workspace)));
                // Before the sub-agent tool, which is always last.
                tools.splice(tools.len() - 1..tools.len() - 1, extra(&workspace));
                ChatTools::new(tools).unwrap()
            });
        let hold = Arc::new(Hold::default());
        Self {
            job: job_over(
                &settings, &chat, &link, &workspace, &data, &hold, feed, tool_wait,
            ),
            data,
            settings,
            workspace,
            chat,
            link,
            host,
            hold,
            bodies,
            repos,
        }
    }

    /// A second job over this rig, as the agent opens one at its next start.
    pub fn open_job(&self, feed: Option<Feed>) -> SetupJob {
        job_over(
            &self.settings,
            &self.chat,
            &self.link,
            &self.workspace,
            &self.data,
            &self.hold,
            feed,
            None,
        )
    }

    pub fn read(&self, path: &str) -> Option<String> {
        self.workspace.read_whole(path, 1 << 20).ok()
    }

    pub fn bodies(&self) -> Vec<Value> {
        self.bodies
            .lock()
            .unwrap()
            .iter()
            .map(|body| serde_json::from_str(body).unwrap())
            .collect()
    }
}
