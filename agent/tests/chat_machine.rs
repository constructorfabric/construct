//! The read-only tools of the user's machine through the whole chat loop: a real chat with `bash`
//! and the four read tools, over a fixture repository and a real workspace folder, answering a
//! scripted fake provider.
#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use construct_agent::chat::Chat;
use construct_agent::chat::machine::machine_tools;
use construct_agent::chat::tool::ChatTools;
use construct_agent::ipc::Server;
use construct_agent::settings::{Settings, Update};
use construct_agent::workspace::{DIR_NAME, Workspace};
use serde_json::{Value, json};

mod common;

use common::scripted::{Turn, provider};
use common::stand::{connect, deadlines};

static NEXT: AtomicU32 = AtomicU32::new(0);

struct Stand {
    base: PathBuf,
    workspace: PathBuf,
    socket: PathBuf,
    bodies: Arc<Mutex<Vec<String>>>,
    _rt: tokio::runtime::Runtime,
}

impl Stand {
    fn new(turns: Vec<Turn>) -> Self {
        let base = std::env::temp_dir().join(format!(
            "cb-chat-machine-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let (data, socket) = (base.join("data"), base.join("run").join("agent.sock"));
        let (addr, bodies) = provider(turns);
        let settings = Settings::open(&base.join("config")).unwrap();
        let model = json!({ "model": {
            "base_url": format!("http://127.0.0.1:{}/v1", addr.port()), "model": "m", "api_key": "k" } });
        settings
            .apply(Update::parse(&model.to_string()).unwrap())
            .unwrap();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let workspace = Arc::new(Workspace::open(&data).unwrap());
        let tools = machine_tools(&workspace);
        let chat = Chat::new(settings.clone(), &data, rt.handle().clone(), deadlines())
            .unwrap()
            .with_tools(ChatTools::new(tools).unwrap());
        let server = Server::bind(&socket)
            .unwrap()
            .with_settings(settings)
            .with_chat(chat);
        thread::spawn(move || server.serve());
        Self {
            workspace: data.join(DIR_NAME),
            base,
            socket,
            bodies,
            _rt: rt,
        }
    }

    /// Sends one message and returns the events of the answer, however it ends.
    fn say(&self, message: &str) -> Vec<Value> {
        connect(
            &self.socket,
            &format!("chat-send {}", json!({ "message": message })),
        )
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

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=Tester",
            "-c",
            "user.email=tester@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A word that is in every file that holds a key or a credential, so a result that has it leaked.
const SECRET_WORD: &str = "LEAKWORD";

/// Files that hold keys or credentials, as the tools see them: all are committed.
const SECRET_FILES: [&str; 11] = [
    ".env",
    ".ENV",
    "id_rsa",
    "KEY.PEM",
    "deploy.pem",
    ".vault-token",
    ".my.cnf",
    "server.jks",
    "a.gpg",
    "credentials.json",
    "secrets.yaml",
];

/// A repository with one remote, two branches, a tag and three commits, the last of them holding
/// key and credential files; a file outside it; a repository that holds a folder of its own; and a
/// folder that is no repository. All in a folder of their own that is removed when this is dropped.
struct Fixture {
    base: PathBuf,
    repo: PathBuf,
    /// A folder inside a repository, which is the only folder the user listed.
    inner: PathBuf,
    /// A folder with two folders, that is no repository.
    plain: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        // As the tools see it: macOS keeps the temporary folder behind a link.
        let temp = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        let base = temp.join(format!(
            "cb-machine-fixture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let repo = base.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        git(
            &repo,
            &[
                "remote",
                "add",
                "origin",
                "https://example.com/team/app.git",
            ],
        );
        git(&repo, &["config", "skip.word", SECRET_WORD]);
        std::fs::write(repo.join("README.md"), "# App\nThe first notes.\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "Start the app"]);
        git(&repo, &["branch", "feature-x"]);
        git(&repo, &["tag", "v1"]);
        std::fs::write(repo.join("notes.txt"), "a plain note\n").unwrap();
        std::fs::write(repo.join("data.json"), "{\"a\":42}\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "Add the notes"]);
        std::fs::write(repo.join("settings.txt"), "a plain setting\n").unwrap();
        for name in SECRET_FILES {
            std::fs::write(repo.join(name), format!("{SECRET_WORD}\n")).unwrap();
        }
        std::fs::create_dir_all(repo.join(".ssh")).unwrap();
        std::fs::write(
            repo.join(".ssh").join("config"),
            format!("Host {SECRET_WORD}\n"),
        )
        .unwrap();
        std::fs::create_dir_all(repo.join(".SSH")).unwrap();
        std::fs::write(
            repo.join(".SSH").join("config"),
            format!("Host {SECRET_WORD}\n"),
        )
        .unwrap();
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        std::fs::write(repo.join("sub").join("readme.txt"), "a plain sub note\n").unwrap();
        std::fs::write(repo.join("sub").join(".env"), format!("{SECRET_WORD}\n")).unwrap();
        git(&repo, &["add", "-f", "."]);
        git(&repo, &["commit", "-q", "-m", "Add the settings"]);

        std::fs::write(base.join("outside.txt"), "outside the folders\n").unwrap();
        std::os::unix::fs::symlink(base.join("outside.txt"), repo.join("link.txt")).unwrap();

        let inner = base.join("outer").join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        git(&base.join("outer"), &["init", "-q"]);
        std::fs::write(base.join("outer").join("top.txt"), "OUTERWORD\n").unwrap();
        std::fs::write(inner.join("inner.txt"), "inner\n").unwrap();
        git(&base.join("outer"), &["add", "."]);
        git(&base.join("outer"), &["commit", "-q", "-m", "Outer commit"]);

        let plain = base.join("plain");
        for side in ["a", "b"] {
            std::fs::create_dir_all(plain.join(side)).unwrap();
            std::fs::write(plain.join(side).join(".env"), format!("{SECRET_WORD}\n")).unwrap();
            std::fs::write(plain.join(side).join("x.txt"), format!("{side}\n")).unwrap();
        }
        Self {
            base,
            repo,
            inner,
            plain,
        }
    }

    fn at(&self, name: &str) -> String {
        self.repo.join(name).to_string_lossy().into_owned()
    }

    /// `command` in the repository.
    fn bash(&self, command: &str) -> (&'static str, Value) {
        in_folder(&self.repo, command)
    }
}

fn in_folder(folder: &Path, command: &str) -> (&'static str, Value) {
    ("bash", json!({ "command": command, "directory": folder }))
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// Runs one turn with `calls` over the fixture, with its repository as the only folder of the
/// user, and returns the results the model got back, in order, and the stand.
fn run(fixture: &Fixture, calls: Vec<(&'static str, Value)>) -> (Vec<String>, Stand) {
    run_in(&[&fixture.repo], calls)
}

/// [`run`] with `folders` as the folders of the user.
fn run_in(folders: &[&Path], calls: Vec<(&'static str, Value)>) -> (Vec<String>, Stand) {
    let count = calls.len();
    let stand = Stand::new(vec![Turn::Calls(calls), Turn::Text("done")]);
    let list = json!({ "folders": folders.iter().map(|f| json!({ "folder": f, "remotes": [] })).collect::<Vec<_>>() });
    std::fs::create_dir_all(stand.workspace.join("history")).unwrap();
    std::fs::write(
        stand.workspace.join("history").join("folders.json"),
        list.to_string(),
    )
    .unwrap();
    let events = stand.say("look at my machine");
    assert_eq!(
        events.last().unwrap(),
        &json!({ "event": "done" }),
        "{events:?}"
    );
    let bodies = stand.bodies();
    assert_eq!(bodies.len(), 2);
    let results: Vec<String> = messages(&bodies[1])
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["content"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(results.len(), count, "{results:?}");
    (results, stand)
}

fn messages(body: &Value) -> Vec<Value> {
    body["messages"].as_array().unwrap().clone()
}

fn names(body: &Value) -> Vec<String> {
    body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap().to_string())
        .collect()
}

/// Checks every `(command, what its error says)` against the results, in order.
fn assert_refused(commands: &[(&str, &str)], results: &[String]) {
    for ((command, said), result) in commands.iter().zip(results) {
        assert!(
            result.starts_with("Error: ") && result.contains(said),
            "{command}: {result}"
        );
    }
}

#[test]
fn the_remote_the_branches_and_the_log_come_back_through_bash() {
    let f = Fixture::new();
    let (results, stand) = run(
        &f,
        vec![
            f.bash("git remote -v"),
            f.bash("git branch"),
            f.bash("git log --oneline"),
            f.bash("git status --short"),
        ],
    );
    assert!(
        results[0].contains("origin\thttps://example.com/team/app.git (fetch)"),
        "{}",
        results[0]
    );
    assert!(
        results[1].contains("* main") && results[1].contains("feature-x"),
        "{}",
        results[1]
    );
    let log: Vec<&str> = results[2].lines().collect();
    assert_eq!(log.len(), 3, "{log:?}");
    assert!(
        log[0].ends_with("Add the settings")
            && log[1].ends_with("Add the notes")
            && log[2].ends_with("Start the app"),
        "{log:?}"
    );
    assert!(results[3].contains("?? link.txt"), "{}", results[3]);
    // The model is offered the tools in a fixed order.
    assert_eq!(
        names(&stand.bodies()[0]),
        [
            "bash",
            "read_file",
            "list_folder",
            "find_files",
            "search_text"
        ]
    );
}

#[test]
fn the_instruction_names_every_tool() {
    let stand = Stand::new(vec![]);
    for tool in machine_tools(&Arc::new(
        Workspace::open(&stand.base.join("data")).unwrap(),
    )) {
        assert!(
            construct_agent::chat::machine::INSTRUCTION.contains(&format!("`{}`", tool.name())),
            "{}",
            tool.name()
        );
    }
}

#[test]
fn the_allowed_programs_work() {
    let f = Fixture::new();
    let (results, _) = run(
        &f,
        vec![
            f.bash("find . -name '*.txt' -type f -not -path './.git/*'"),
            f.bash("jq .a data.json"),
            f.bash("jq -r .a data.json"),
            f.bash("git tag -l"),
            f.bash("git show --stat HEAD~1"),
            f.bash("git diff HEAD~2 HEAD~1 --stat"),
            f.bash("git log -n 1 --format=%s"),
            f.bash("git log -n1 --oneline -- README.md"),
            f.bash("git rev-parse --show-toplevel"),
            f.bash("git ls-files"),
            f.bash("git describe --tags"),
        ],
    );
    let found: Vec<&str> = results[0].lines().collect();
    assert!(
        found.contains(&f.at("notes.txt").as_str())
            && found.contains(&f.at("settings.txt").as_str()),
        "{found:?}"
    );
    assert_eq!(results[1], "42\n");
    assert_eq!(results[2], "42\n");
    assert_eq!(results[3], "v1\n");
    assert!(
        results[4].contains("Add the notes") && results[4].contains("notes.txt"),
        "{}",
        results[4]
    );
    assert!(
        results[5].contains("notes.txt") && results[5].contains("data.json"),
        "{}",
        results[5]
    );
    assert_eq!(results[6], "Add the settings\n");
    assert!(results[7].ends_with("Start the app\n"), "{}", results[7]);
    assert_eq!(results[8].trim(), f.repo.to_string_lossy());
    assert!(
        results[9].contains("README.md") && results[9].contains("notes.txt"),
        "{}",
        results[9]
    );
    assert!(results[10].starts_with("v1-2-g"), "{}", results[10]);
}

#[test]
fn the_read_tools_work_inside_the_folder_and_leave_out_keys_and_credentials() {
    let f = Fixture::new();
    let (results, _) = run(
        &f,
        vec![
            ("read_file", json!({ "path": f.at("notes.txt") })),
            ("list_folder", json!({ "path": f.repo })),
            (
                "find_files",
                json!({ "pattern": "**/*.md", "path": f.repo }),
            ),
            ("search_text", json!({ "pattern": "first", "path": f.repo })),
            f.bash("grep -rn plain ."),
            f.bash("cat notes.txt"),
            f.bash("ls -a"),
        ],
    );
    assert_eq!(results[0], "     1\ta plain note");
    let listed: Vec<&str> = results[1].lines().collect();
    assert_eq!(
        listed,
        [
            ".git/",
            "README.md",
            "data.json",
            "link.txt",
            "notes.txt",
            "settings.txt",
            "sub/"
        ],
        "{listed:?}"
    );
    assert_eq!(results[2], f.at("README.md"));
    assert_eq!(
        results[3],
        format!("{}:2:The first notes.", f.at("README.md"))
    );
    assert!(
        results[4].contains("notes.txt:1:a plain note"),
        "{}",
        results[4]
    );
    assert_eq!(results[5], "a plain note\n");
    assert!(results[6].contains("notes.txt"), "{}", results[6]);
}

#[test]
fn a_command_that_is_not_allowed_gets_a_plain_error() {
    let f = Fixture::new();
    let commands = [
        ("rm -rf notes.txt", "`rm` is not allowed"),
        ("curl https://example.com", "`curl` is not allowed"),
        ("/bin/rm notes.txt", "is not allowed"),
        ("git push origin main", "`git push` is not allowed"),
        ("git config user.name x", "`git config` is not allowed"),
        ("git -c core.pager=x log", "`git -c` is not allowed"),
        ("ls | wc", "no shell"),
        ("cat notes.txt > copy.txt", "no shell"),
        ("ls; rm notes.txt", "no shell"),
        ("echo $HOME", "no shell"),
    ];
    let (results, _) = run(&f, commands.iter().map(|(c, _)| f.bash(c)).collect());
    assert_refused(&commands, &results);
    assert!(results[0].contains("git, ls, cat, grep, find, jq"));
    assert!(f.repo.join("notes.txt").exists() && !f.repo.join("copy.txt").exists());
}

#[test]
fn a_command_that_would_write_gets_a_plain_error_and_nothing_changes() {
    let f = Fixture::new();
    let commands = [
        (
            "git commit -m change --allow-empty",
            "`git commit` is not allowed",
        ),
        ("git branch created", "`git branch` can only list"),
        ("git branch -D feature-x", "`git branch` can only list"),
        ("git tag v2", "`git tag` can only list"),
        (
            "git remote add other https://example.com/x.git",
            "`git remote` can run",
        ),
        (
            "git remote set-url origin https://example.com/y.git",
            "`git remote` can run",
        ),
        ("git log --output=out.txt", "`--output` is not allowed"),
        ("git log --outp=out.txt", "`--outp` is not allowed"),
        (
            "git diff --no-index notes.txt README.md",
            "`--no-index` is not allowed",
        ),
        ("find . -delete", "`-delete` is not allowed"),
        ("find . -exec rm x +", "`-exec` is not allowed"),
        ("find . -fprint out.txt", "`-fprint` is not allowed"),
        ("grep -f notes.txt README.md", "`-f` is not allowed"),
        ("ls -R --time-style=x", "`--time-style=x` is not allowed"),
        ("jq -n env", "reads the environment"),
        ("jq -n '$ENV'", "reads the environment"),
        ("jq --rawfile a notes.txt .", "`--rawfile` is not allowed"),
    ];
    let (results, _) = run(&f, commands.iter().map(|(c, _)| f.bash(c)).collect());
    assert_refused(&commands, &results);
    for name in ["out.txt", "created"] {
        assert!(!f.repo.join(name).exists(), "{name}");
    }
    assert_eq!(git(&f.repo, &["tag", "--list"]), "v1");
    assert!(git(&f.repo, &["remote", "-v"]).contains("example.com/team/app.git"));
    assert!(!git(&f.repo, &["remote"]).contains("other"));
    let branches = git(&f.repo, &["branch", "--list"]);
    assert!(
        branches.contains("feature-x") && !branches.contains("created"),
        "{branches}"
    );
}

#[test]
fn a_path_outside_the_allowed_folders_gets_a_plain_error() {
    let f = Fixture::new();
    let outside = f.base.join("outside.txt").to_string_lossy().into_owned();
    let parent = f.base.to_string_lossy().into_owned();
    let calls = vec![
        ("read_file", json!({ "path": outside })),
        ("read_file", json!({ "path": "/etc/passwd" })),
        ("read_file", json!({ "path": f.at("link.txt") })),
        ("read_file", json!({ "path": f.at("../outside.txt") })),
        ("read_file", json!({ "path": "notes.txt" })),
        ("list_folder", json!({ "path": parent })),
        ("find_files", json!({ "pattern": "*", "path": "/" })),
        ("search_text", json!({ "pattern": "root", "path": "/etc" })),
        f.bash("cat ../outside.txt"),
        f.bash("cat /etc/passwd"),
        f.bash("ls /"),
        f.bash("git show HEAD --no-patch -- ../outside.txt"),
        ("bash", json!({ "command": "ls", "directory": "/etc" })),
        ("bash", json!({ "command": "ls", "directory": parent })),
    ];
    let (results, _) = run(&f, calls);
    let message = "Error: that path is outside the folders you may read";
    for (at, result) in results.iter().enumerate() {
        let expected = match at {
            4 => "Error: give the full path, starting with /",
            11 => "Error: a path in a git command must stay inside the folder",
            _ => message,
        };
        assert_eq!(result, expected, "call {at}");
    }
}

#[test]
fn keys_and_credentials_are_refused_and_never_read() {
    let f = Fixture::new();
    let blob = git(&f.repo, &["rev-parse", "HEAD:.env"]);
    let calls = vec![
        ("read_file", json!({ "path": f.at(".env") })),
        ("read_file", json!({ "path": f.at("id_rsa") })),
        ("list_folder", json!({ "path": f.at(".ssh") })),
        (
            "search_text",
            json!({ "pattern": "Host", "path": f.at(".ssh/config") }),
        ),
        f.bash("cat .env"),
        f.bash("cat .ssh/config"),
        f.bash("git log -p -- .env"),
        f.bash("git show HEAD:id_rsa"),
        f.bash("grep -rn Host .ssh"),
        f.bash("grep -n LEAKWORD .ENV"),
    ];
    let (results, _) = run(&f, calls);
    let message = "Error: that path may hold keys or credentials and is not available";
    for result in &results[..9] {
        assert_eq!(result, message);
    }
    assert_eq!(results[9], message);

    // Search never reads them, in any case, and neither does `git`, by any route.
    let calls = vec![
        f.bash("grep -rn LEAKWORD"),
        f.bash("grep -rn LEAKWORD ."),
        f.bash("git log -p"),
        f.bash("git show HEAD"),
        f.bash("git diff HEAD~1 HEAD"),
        f.bash("git log -p -- '*.p?m'"),
        f.bash("git log -p -n1 -- deploy.pem credentials.json"),
        f.bash("git log -p -L1,1:.env"),
        f.bash("git ls-files -s"),
        f.bash(&format!("git show {blob}")),
        f.bash(&format!("git diff {blob} {blob}")),
        f.bash("git show HEAD:settings.txt"),
        f.bash("git log -p --since=2000-01-01"),
    ];
    let (results, _) = run(&f, calls);
    for (at, result) in results.iter().enumerate() {
        assert!(!result.contains(SECRET_WORD), "call {at}: {result}");
    }
    assert_eq!(results[0], "(no output)\n[exit status 1]");
    assert_eq!(results[1], "(no output)\n[exit status 1]");
    assert!(results[2].contains("a plain setting"), "{}", results[2]);
    assert!(results[3].contains("a plain setting"), "{}", results[3]);
    assert!(results[4].contains("a plain setting"), "{}", results[4]);
    assert_eq!(results[5], "(no output)");
    assert_eq!(results[6], message);
    assert!(results[12].contains("a plain setting"), "{}", results[12]);
    assert!(
        results[7].starts_with("Error: the option `-L"),
        "{}",
        results[7]
    );
    assert!(results[9].contains("is not a commit"), "{}", results[9]);
    assert!(results[10].contains("is not a commit"), "{}", results[10]);
    assert!(results[11].contains("is not a commit"), "{}", results[11]);
    // Only the names of the files show, as `ls` shows them; the files hold nothing else.
    assert!(results[8].contains("100644"), "{}", results[8]);
}

#[test]
fn grep_skips_the_git_folder_and_every_name_that_holds_keys_in_any_case() {
    let f = Fixture::new();
    std::fs::write(
        f.repo.join("visible.txt"),
        format!("{SECRET_WORD} is here\n"),
    )
    .unwrap();
    let (results, _) = run(&f, vec![f.bash("grep -rn LEAKWORD")]);
    assert_eq!(results[0], "visible.txt:1:LEAKWORD is here\n");
}

#[test]
fn git_runs_only_in_a_repository_inside_the_folders_of_the_user() {
    let f = Fixture::new();
    let (results, _) = run_in(
        &[&f.inner, &f.plain],
        vec![
            in_folder(&f.inner, "git log -p"),
            in_folder(&f.inner, "git show HEAD"),
            in_folder(&f.inner, "git ls-files"),
            in_folder(&f.inner, "git status"),
            in_folder(&f.plain, "git diff a b"),
            in_folder(&f.plain, "git log"),
            in_folder(&f.inner, "ls"),
        ],
    );
    let said = "Error: this folder is not inside a git repository that is in your folders";
    for result in &results[..6] {
        assert_eq!(result, said);
    }
    assert_eq!(results[6], "inner.txt\n");
}

#[test]
fn a_long_output_is_cut() {
    let f = Fixture::new();
    let long = "a line of the long file\n".repeat(20_000);
    std::fs::write(f.repo.join("long.txt"), &long).unwrap();
    let (results, _) = run(
        &f,
        vec![
            ("read_file", json!({ "path": f.at("long.txt") })),
            f.bash("cat long.txt"),
            ("search_text", json!({ "pattern": "long", "path": f.repo })),
        ],
    );
    let limit = 64 * 1024;
    for result in &results {
        assert!(result.len() <= limit, "{}", result.len());
    }
    let note = results[0].lines().last().unwrap();
    assert!(
        note.starts_with("(The result was cut. Continue with offset ")
            && note.ends_with(" to read the rest.)"),
        "{note}"
    );
    assert!(results[1].ends_with(" [cut]"));
    assert!(results[1].starts_with("a line of the long file"));
    assert!(results[2].contains("(There are more matches than are shown.)"));
}

#[test]
fn secrets_are_masked_in_files_in_search_results_and_in_command_output() {
    let f = Fixture::new();
    let text = "api_key = sk-FAKEFAKEFAKEFAKE1234\npassword: hunter2hunter2\n\
                see https://user:pa55word@example.com/repo.git\n\
                -----BEGIN PRIVATE KEY-----\nFAKEKEYMATERIAL\n-----END PRIVATE KEY-----\n";
    std::fs::write(f.repo.join("config.txt"), text).unwrap();
    git(&f.repo, &["add", "config.txt"]);
    git(
        &f.repo,
        &[
            "commit",
            "-q",
            "-m",
            "Set token=ghp_FAKEFAKEFAKEFAKEFAKE1234",
        ],
    );
    let (results, _) = run(
        &f,
        vec![
            ("read_file", json!({ "path": f.at("config.txt") })),
            (
                "search_text",
                json!({ "pattern": "password", "path": f.repo }),
            ),
            f.bash("cat config.txt"),
            f.bash("git log -n 1"),
        ],
    );
    for result in &results {
        for secret in [
            "sk-FAKE",
            "hunter2",
            "pa55word",
            "FAKEKEYMATERIAL",
            "ghp_FAKE",
        ] {
            assert!(!result.contains(secret), "{secret} in {result}");
        }
    }
    assert!(
        results[0].contains("[secret removed]") && results[0].contains("[private key removed]"),
        "{}",
        results[0]
    );
    assert!(results[1].contains("[secret removed]"), "{}", results[1]);
    assert!(results[2].contains("[secret removed]"));
    assert!(results[3].contains("[secret removed]"), "{}", results[3]);
}

#[test]
fn git_leaves_out_secret_names_from_a_subfolder_and_from_staged_changes() {
    let f = Fixture::new();
    std::fs::write(f.repo.join(".env"), format!("{SECRET_WORD} staged\n")).unwrap();
    std::fs::write(f.repo.join("notes.txt"), "a staged plain note\n").unwrap();
    git(&f.repo, &["add", "-f", ".env", "notes.txt"]);
    let sub = f.repo.join("sub");
    let calls = vec![
        in_folder(&sub, "git log -p --all"),
        in_folder(&sub, "git log -p -- ':(top)'"),
        in_folder(&sub, "git log -p -- ':/'"),
        in_folder(&sub, "git diff --cached"),
        in_folder(&sub, "git diff HEAD~1 HEAD"),
        f.bash("git status -v"),
        f.bash("git status -vv"),
        f.bash("git status --verbose"),
        f.bash("git status --verb"),
        f.bash("git status --short"),
        f.bash("git log -p --full-diff -- notes.txt"),
        f.bash("git log -p --full-d -- notes.txt"),
        f.bash("git show --full-diff"),
        f.bash("git diff --full-diff HEAD~1 HEAD"),
        f.bash("git log -p --submodule=diff"),
        f.bash("git log -p --subm"),
    ];
    let (results, _) = run(&f, calls);
    for (at, result) in results.iter().enumerate() {
        assert!(!result.contains(SECRET_WORD), "call {at}: {result}");
    }
    for at in [0, 1, 2] {
        assert!(
            results[at].contains("a plain sub note"),
            "{at}: {}",
            results[at]
        );
    }
    assert!(results[3].contains("a staged plain note"), "{}", results[3]);
    assert!(results[4].contains("a plain sub note"), "{}", results[4]);
    for result in &results[5..9] {
        assert!(
            result.starts_with("Error: `git status` can run with"),
            "{result}"
        );
    }
    assert!(results[9].contains("M  notes.txt"), "{}", results[9]);
    for result in &results[10..16] {
        assert!(
            result.starts_with("Error: the option `--") && result.ends_with("is not allowed"),
            "{result}"
        );
    }
}

#[test]
fn options_that_read_files_run_programs_or_check_signatures_are_refused() {
    let f = Fixture::new();
    let commands = [
        ("git log -Oorder.txt", "`-Oorder.txt` is not allowed"),
        ("git ls-files -X ignore.txt", "`-X` is not allowed"),
        ("git log --format=%GG", "signature checks are not allowed"),
        (
            "git log --show-signature",
            "`--show-signature` is not allowed",
        ),
        (
            "jq 'import \"a\" as a; .' data.json",
            "reads the environment",
        ),
        ("jq 'include \"a\"; .' data.json", "reads the environment"),
        ("jq 'modulemeta' data.json", "reads the environment"),
        (
            "git log -n1 -- ../outside.txt",
            "must stay inside the folder",
        ),
        ("git log /etc/passwd", "must stay inside the folder"),
    ];
    let (results, _) = run(&f, commands.iter().map(|(c, _)| f.bash(c)).collect());
    assert_refused(&commands, &results);
}

#[test]
fn programs_named_in_the_repository_configuration_are_never_run() {
    let f = Fixture::new();
    let marker = f.base.join("ran");
    let script = f.base.join("hook.sh");
    std::fs::write(
        &script,
        format!("#!/bin/sh\ntouch {}\nexit 0\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let script = script.to_string_lossy().into_owned();
    git(&f.repo, &["config", "core.fsmonitor", &script]);
    git(&f.repo, &["config", "diff.external", &script]);
    git(&f.repo, &["config", "diff.cfg.textconv", &script]);
    std::fs::write(f.repo.join(".gitattributes"), "*.txt diff=cfg\n").unwrap();
    // Without the guards, git does run them.
    git(&f.repo, &["status"]);
    assert!(marker.exists(), "the fixture does not trigger the program");
    std::fs::remove_file(&marker).unwrap();
    git(&f.repo, &["diff", "HEAD~1", "HEAD"]);
    assert!(
        marker.exists(),
        "the fixture does not trigger the diff program"
    );
    std::fs::remove_file(&marker).unwrap();

    let (results, _) = run(
        &f,
        vec![
            f.bash("git status"),
            f.bash("git log -p -n1"),
            f.bash("git show HEAD~1"),
            f.bash("git diff HEAD~1 HEAD"),
            f.bash("git diff"),
        ],
    );
    assert!(!marker.exists(), "a configured program ran");
    assert!(results[1].contains("a plain setting"), "{}", results[1]);
    assert!(results[3].contains("a plain setting"), "{}", results[3]);
}

#[test]
fn a_walk_stops_at_the_most_files_it_looks_at() {
    let f = Fixture::new();
    let many = f.plain.join("many");
    std::fs::create_dir_all(&many).unwrap();
    for at in 0..50_001 {
        std::fs::write(many.join(format!("f{at}.dat")), "").unwrap();
    }
    let (results, _) = run_in(
        &[&f.plain],
        vec![(
            "find_files",
            json!({ "pattern": "**/f1.dat", "path": f.plain }),
        )],
    );
    assert!(
        results[0].starts_with(&many.join("f1.dat").to_string_lossy().into_owned())
            && results[0].contains("The search stopped early"),
        "{}",
        results[0]
    );
}

#[test]
fn options_that_add_objects_or_references_as_revisions_are_refused() {
    let f = Fixture::new();
    let blob = git(&f.repo, &["rev-parse", "HEAD:.env"]);
    git(&f.repo, &["update-ref", "refs/tags/blobtag", &blob]);
    let commands = [
        ("git show --indexed-objects", "not allowed for `git show`"),
        ("git show --tags", "not allowed for `git show`"),
        ("git show --all", "not allowed for `git show`"),
        ("git show --branches", "not allowed for `git show`"),
        ("git show --remotes", "not allowed for `git show`"),
        ("git show --objects", "not allowed for `git show`"),
        ("git log --objects", "not allowed for `git log`"),
        ("git log --glob=refs/tags/*", "not allowed for `git log`"),
        ("git log --exclude=x", "`--exclude` is not allowed"),
        ("git log --alternate-refs", "not allowed for `git log`"),
        ("git log --reflog", "not allowed for `git log`"),
        ("git log -g", "not allowed for `git log`"),
        ("git log --walk-reflogs", "not allowed for `git log`"),
        ("git log --stdin", "not allowed for `git log`"),
        ("git log --no-walk", "not allowed for `git log`"),
        ("git diff --all", "not allowed for `git diff`"),
        (
            "git ls-files --recurse-submodules",
            "`--recurse-submodules` is not allowed",
        ),
        ("git diff --tags", "not allowed for `git diff`"),
    ];
    let mut calls: Vec<_> = commands.iter().map(|(c, _)| f.bash(c)).collect();
    calls.push(f.bash("git log -p --all --tags --branches --remotes"));
    let (results, _) = run(&f, calls);
    assert_refused(&commands, &results);
    // Where a reference points at a blob, `log` still prints no blob.
    let last = results.last().unwrap();
    assert!(
        !last.contains(SECRET_WORD) && last.contains("a plain setting"),
        "{last}"
    );
}

#[test]
fn a_repository_that_names_programs_for_git_to_run_is_not_read() {
    let f = Fixture::new();
    let marker = f.base.join("ran");
    let script = f.base.join("hook.sh");
    std::fs::write(
        &script,
        format!("#!/bin/sh\ntouch {}\ncat\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let script = script.to_string_lossy().into_owned();
    // A filter that runs when git compares a changed file.
    git(&f.repo, &["config", "filter.x.clean", &script]);
    std::fs::write(f.repo.join(".gitattributes"), "notes.txt filter=x\n").unwrap();
    std::fs::write(f.repo.join("notes.txt"), "changed\n").unwrap();
    git(&f.repo, &["diff"]);
    assert!(marker.exists(), "the fixture does not trigger the filter");
    std::fs::remove_file(&marker).unwrap();

    let (results, _) = run(
        &f,
        vec![f.bash("git diff"), f.bash("git status"), f.bash("git log")],
    );
    let said = "Error: the configuration of this repository names programs for git to run";
    for result in &results {
        assert!(result.starts_with(said), "{result}");
    }
    assert!(!marker.exists(), "the filter ran");
    // The same holds for every key of the list.
    for (key, value) in [
        ("gpg.format", "x509"),
        ("gpg.x509.program", "/bin/true"),
        ("core.hooksPath", "/tmp"),
        ("core.sshCommand", "/bin/true"),
        ("core.worktree", "/tmp"),
    ] {
        let f = Fixture::new();
        git(&f.repo, &["config", key, value]);
        let (results, _) = run(&f, vec![f.bash("git log")]);
        assert!(results[0].starts_with(said), "{key}: {}", results[0]);
    }
}

#[test]
fn a_signature_program_named_by_the_repository_is_not_run() {
    let f = Fixture::new();
    let marker = f.base.join("ran");
    let script = f.base.join("verify.sh");
    std::fs::write(&script, format!("#!/bin/sh\ntouch {}\n", marker.display())).unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let tree = git(&f.repo, &["rev-parse", "HEAD^{tree}"]);
    let parent = git(&f.repo, &["rev-parse", "HEAD"]);
    let commit = format!(
        "tree {tree}\nparent {parent}\nauthor A <a@example.com> 1700000000 +0000\n\
         committer A <a@example.com> 1700000000 +0000\ngpgsig -----BEGIN SIGNED MESSAGE-----\n \n \
         fake\n -----END SIGNED MESSAGE-----\n\nsigned\n"
    );
    let mut child = Command::new("git")
        .args(["hash-object", "-t", "commit", "-w", "--stdin"])
        .current_dir(&f.repo)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), commit.as_bytes()).unwrap();
    let sha = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
    git(&f.repo, &["update-ref", "HEAD", sha.trim()]);
    git(&f.repo, &["config", "log.showSignature", "true"]);
    git(&f.repo, &["config", "gpg.format", "x509"]);
    git(
        &f.repo,
        &["config", "gpg.x509.program", &script.to_string_lossy()],
    );
    git(&f.repo, &["log", "-1"]);
    assert!(marker.exists(), "the fixture does not trigger the program");
    std::fs::remove_file(&marker).unwrap();
    let (results, _) = run(&f, vec![f.bash("git log -1")]);
    assert!(
        results[0].starts_with("Error: the configuration"),
        "{}",
        results[0]
    );
    assert!(!marker.exists(), "the signature program ran");
}

#[test]
fn a_submodule_diff_in_the_configuration_does_not_print_secret_files() {
    let f = Fixture::new();
    let source = f.base.join("sm-source");
    std::fs::create_dir_all(&source).unwrap();
    git(&source, &["init", "-q"]);
    std::fs::write(source.join("x.txt"), "x\n").unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-q", "-m", "First"]);
    let allow = ["-c", "protocol.file.allow=always"];
    let source_path = source.to_string_lossy().into_owned();
    git(
        &f.repo,
        &[
            allow[0],
            allow[1],
            "submodule",
            "add",
            "-q",
            &source_path,
            "sm",
        ],
    );
    git(&f.repo, &["commit", "-q", "-m", "Add the submodule"]);
    std::fs::write(source.join(".env"), format!("{SECRET_WORD}\n")).unwrap();
    git(&source, &["add", "-f", "."]);
    git(&source, &["commit", "-q", "-m", "Second"]);
    git(&f.repo.join("sm"), &[allow[0], allow[1], "pull", "-q"]);
    git(&f.repo, &["add", "sm"]);
    git(&f.repo, &["commit", "-q", "-m", "Update the submodule"]);
    git(&f.repo, &["config", "diff.submodule", "diff"]);
    // Plain git prints the inner diff, secret file included.
    assert!(git(&f.repo, &["log", "-p", "-n1"]).contains(SECRET_WORD));

    let (results, _) = run(
        &f,
        vec![
            f.bash("git log -p -n1"),
            f.bash("git show HEAD"),
            f.bash("git diff HEAD~1 HEAD"),
        ],
    );
    for result in &results {
        assert!(!result.contains(SECRET_WORD), "{result}");
    }
    assert!(
        results[0].contains("Update the submodule"),
        "{}",
        results[0]
    );
}

#[test]
fn a_command_may_name_ten_revisions_and_no_more() {
    let f = Fixture::new();
    let ten = format!("git log --oneline {}", ["HEAD"; 10].join(" "));
    let eleven = format!("git log --oneline {}", ["HEAD"; 11].join(" "));
    let (results, _) = run(&f, vec![f.bash(&ten), f.bash(&eleven)]);
    assert!(results[0].contains("Add the settings"), "{}", results[0]);
    assert_eq!(results[1], "Error: too many revisions in one command");
}

/// A script that makes `marker` when run, as git runs it for a filter, and its path.
fn marker_script(f: &Fixture, name: &str) -> (String, PathBuf) {
    let marker = f.base.join(format!("ran-{name}"));
    let script = f.base.join(format!("{name}.sh"));
    std::fs::write(
        &script,
        format!("#!/bin/sh\ntouch {}\ncat\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    (script.to_string_lossy().into_owned(), marker)
}

/// Makes `notes.txt` dirty and routes it through the filter `x`, so `git diff` and `git status`
/// run the clean filter of the configuration, if there is one.
fn route_notes_through_filter(repo: &Path) {
    std::fs::write(repo.join(".gitattributes"), "notes.txt filter=x\n").unwrap();
    std::fs::write(repo.join("notes.txt"), "changed\n").unwrap();
}

#[test]
fn a_configuration_that_cannot_be_read_in_full_is_not_trusted() {
    let f = Fixture::new();
    let (script, marker) = marker_script(&f, "filter");
    // More than the 1 MiB read from an output, with the filter after it.
    let mut config = std::fs::read_to_string(f.repo.join(".git").join("config")).unwrap();
    config.push_str("[padding]\n");
    for at in 0..1500 {
        let _ = writeln!(config, "\tkey{at} = {}", "x".repeat(1000));
    }
    let _ = write!(config, "[filter \"x\"]\n\tclean = {script}\n");
    std::fs::write(f.repo.join(".git").join("config"), config).unwrap();
    route_notes_through_filter(&f.repo);
    git(&f.repo, &["diff"]);
    assert!(marker.exists(), "the fixture does not trigger the filter");
    std::fs::remove_file(&marker).unwrap();

    let (results, _) = run(&f, vec![f.bash("git diff"), f.bash("git status")]);
    for result in &results {
        assert_eq!(
            result,
            "Error: the configuration of this repository could not be read in full, so git is not run here"
        );
    }
    assert!(!marker.exists(), "the filter ran");
}

#[test]
fn a_filter_in_an_included_configuration_file_is_found() {
    let f = Fixture::new();
    let (script, marker) = marker_script(&f, "filter");
    let extra = f.base.join("extra.cfg");
    std::fs::write(&extra, format!("[filter \"x\"]\n\tclean = {script}\n")).unwrap();
    git(
        &f.repo,
        &["config", "include.path", &extra.to_string_lossy()],
    );
    route_notes_through_filter(&f.repo);
    git(&f.repo, &["diff"]);
    assert!(marker.exists(), "the fixture does not trigger the filter");
    std::fs::remove_file(&marker).unwrap();

    let (results, _) = run(&f, vec![f.bash("git diff"), f.bash("git status")]);
    for result in &results {
        assert!(
            result.starts_with("Error: the configuration of this repository names programs"),
            "{result}"
        );
    }
    assert!(!marker.exists(), "the filter ran");
}

/// A repository with a dirty submodule whose own configuration runs a clean filter, and the file
/// that filter creates when it runs.
fn repository_with_a_filter_in_a_submodule() -> (Fixture, PathBuf) {
    let f = Fixture::new();
    let source = f.base.join("sm-source");
    std::fs::create_dir_all(&source).unwrap();
    git(&source, &["init", "-q"]);
    std::fs::write(source.join("x.txt"), "x\n").unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-q", "-m", "First"]);
    let source_path = source.to_string_lossy().into_owned();
    git(
        &f.repo,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            &source_path,
            "sm",
        ],
    );
    git(&f.repo, &["commit", "-q", "-m", "Add the submodule"]);
    let (script, marker) = marker_script(&f, "filter");
    let sm = f.repo.join("sm");
    git(&sm, &["config", "filter.evil.clean", &script]);
    std::fs::write(sm.join(".gitattributes"), "* filter=evil\n").unwrap();
    // The same size, so that git has to compare the content, which runs the clean filter.
    std::fs::write(sm.join("x.txt"), "y\n").unwrap();
    (f, marker)
}

#[test]
fn a_filter_in_a_submodule_configuration_is_not_run_by_status_or_diff() {
    let (f, marker) = repository_with_a_filter_in_a_submodule();
    git(&f.repo, &["status", "-s"]);
    assert!(marker.exists(), "the fixture does not trigger the filter");
    std::fs::remove_file(&marker).unwrap();

    let (results, _) = run(&f, vec![f.bash("git status -s"), f.bash("git status")]);
    // Submodules are not looked into, so nothing of theirs shows, but the rest of the status does.
    assert!(
        results[0].contains("link.txt") && !results[0].contains("sm"),
        "{}",
        results[0]
    );
    assert!(!marker.exists(), "the filter of the submodule ran");

    // `.gitmodules` can ask for the submodule to be looked into, which `-c` does not beat.
    git(
        &f.repo,
        &["config", "-f", ".gitmodules", "submodule.sm.ignore", "none"],
    );
    git(&f.repo, &["diff"]);
    assert!(
        marker.exists(),
        "the fixture does not trigger the filter under diff"
    );
    std::fs::remove_file(&marker).unwrap();
    run(&f, vec![f.bash("git diff"), f.bash("git diff HEAD")]);
    assert!(
        !marker.exists(),
        "the filter of the submodule ran under diff"
    );
}

#[test]
fn describe_is_not_asked_to_look_into_a_submodule_for_changes() {
    let (f, marker) = repository_with_a_filter_in_a_submodule();
    git(&f.repo, &["describe", "--dirty", "--always"]);
    assert!(
        marker.exists(),
        "the fixture does not trigger the filter under describe"
    );
    std::fs::remove_file(&marker).unwrap();
    let (results, _) = run(
        &f,
        vec![
            f.bash("git describe --dirty --always"),
            f.bash("git describe --broken --always"),
            f.bash("git describe --dir --always"),
            f.bash("git describe --b --always"),
            f.bash("git describe --di --always"),
            f.bash("git describe --always"),
        ],
    );
    assert!(
        results[..5].iter().all(|r| r.contains("is not allowed")),
        "{results:?}"
    );
    assert!(!results[5].contains("is not allowed"), "{results:?}");
    assert!(
        !marker.exists(),
        "the filter of the submodule ran under describe"
    );
}
