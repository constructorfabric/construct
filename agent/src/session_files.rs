//! The session files of a dashboard setup run: the sessions of a period, written as plain text
//! into the folder `history` of the workspace, one file per session, so the run's file tools can
//! read them, and `history/folders.json`, the distinct working folders with the git remotes found
//! in each folder's `.git/config`.
//!
//! The record store is the only source, and the remotes are read from a file, never by running a
//! program. Left out as no use to a model: tool calls and results, a long instruction block that
//! an earlier session of the run already had, and anything that looks like a secret.
//!
//! The files are the user's own text, so they live only as long as the run. `history` belongs to
//! the run: whatever is under that name when a run starts is replaced, which is how the leftovers
//! of a run that died go. One run at a time holds the folder; the caller starts the next only
//! after it closed or dropped the last.

use std::collections::{BTreeSet, HashSet};
use std::fmt::Write as _;
use std::io::Read;
use std::num::NonZeroU32;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use connectors::{Author, Record, Role, compare_times, minute_of};
use regex::Regex;
use serde::Serialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::records::FileSink;
use crate::workspace::{Error, Workspace};

/// The folder of the session files, inside the workspace.
pub const DIR: &str = "history";
/// The folder list, inside the workspace.
pub const FOLDERS_FILE: &str = "history/folders.json";
/// The period of a run when the caller names none, in days.
const DEFAULT_DAYS: u32 = 30;

/// A turn longer than this (in characters) may be an instruction block that runs repeat.
const LONG_TURN: usize = 2_000;
/// How many leading characters tell one instruction block from another.
const OPENING: usize = 200;
/// The most of a git config file that is read.
const MAX_CONFIG: u64 = 1 << 20;
/// What takes the place of a hidden secret.
const MASK: &str = "[secret removed]";

/// The files of one run. They are removed when this is closed or dropped.
#[derive(Debug)]
pub struct SessionFiles<'a> {
    workspace: &'a Workspace,
    files: Vec<String>,
    /// The bytes of text in the session files.
    bytes: u64,
    removed: bool,
}

#[derive(Serialize, serde::Deserialize)]
struct Remote {
    name: String,
    url: String,
}

#[derive(Serialize, serde::Deserialize)]
pub(crate) struct Folder {
    pub(crate) folder: String,
    /// Empty when the folder is gone from this machine or is not a git repository.
    remotes: Vec<Remote>,
}

/// The content of `history/folders.json`, written here and read by the tools of the machine.
#[derive(Serialize, serde::Deserialize)]
pub(crate) struct Folders {
    pub(crate) folders: Vec<Folder>,
}

impl<'a> SessionFiles<'a> {
    /// Writes the sessions of `sources` whose last record is within `days` days before `now`
    /// (`DEFAULT_DAYS` when none is given), oldest first, and the folder list. Each
    /// session is read from the store when its turn comes, so a session that is still being
    /// written is seen as it is at that moment, and a line that is half written is skipped.
    ///
    /// `go` is asked before each session is read, with the number of session files written so far
    /// and their bytes of text: when it answers `false` the files are removed and nothing is
    /// returned.
    ///
    /// A `history` folder that is already in the workspace is replaced.
    ///
    /// # Errors
    /// The store cannot be read or the workspace cannot be written.
    pub fn write(
        workspace: &'a Workspace,
        sink: &FileSink,
        sources: &[&str],
        days: Option<NonZeroU32>,
        now: OffsetDateTime,
        go: &mut dyn FnMut(usize, u64) -> bool,
    ) -> Result<Option<Self>, Error> {
        workspace.remove_all(DIR)?;
        // From here on the folder may exist, so the drop of `run` removes it on any failure.
        let mut run = Self {
            workspace,
            files: Vec::new(),
            bytes: 0,
            removed: false,
        };
        let cutoff = cutoff(now, days.map_or(DEFAULT_DAYS, NonZeroU32::get))?;
        let mut cleaner = Cleaner::default();
        let mut folders = BTreeSet::new();
        for source in sources {
            let listed = sink.list_sessions(source).map_err(Error::from)?;
            for info in listed.into_iter().rev() {
                if compare_times(&info.last_time, &cutoff).is_lt() {
                    continue;
                }
                if !go(run.files.len(), run.bytes) {
                    return Ok(None);
                }
                let records = sink
                    .read_session(source, &info.session)
                    .map_err(Error::from)?;
                let Some(text) = render(source, &info.session, &records, &mut cleaner) else {
                    continue;
                };
                folders.extend(
                    records
                        .iter()
                        .filter_map(|r| folder_of(r).map(str::to_owned)),
                );
                let name = format!(
                    "{DIR}/{:04}-{}-{}.txt",
                    run.files.len() + 1,
                    plain(source),
                    plain(&info.session)
                );
                workspace.write(&name, &text)?;
                run.bytes += text.len() as u64;
                run.files.push(name);
            }
        }
        let list = Folders {
            folders: folders
                .into_iter()
                .map(|folder| Folder {
                    remotes: remotes_of(Path::new(&folder)),
                    folder,
                })
                .collect(),
        };
        let json = serde_json::to_string_pretty(&list).map_err(|_| Error::NotText)?;
        workspace.write(FOLDERS_FILE, &json)?;
        if !go(run.files.len(), run.bytes) {
            return Ok(None);
        }
        Ok(Some(run))
    }

    /// How many sessions were written.
    #[must_use]
    pub fn sessions(&self) -> usize {
        self.files.len()
    }

    /// The workspace paths of the session files, oldest session first within a source.
    #[cfg(test)]
    fn files(&self) -> &[String] {
        &self.files
    }

    /// Removes the folder, and reports a failure that a drop could only hide.
    ///
    /// # Errors
    /// The folder cannot be removed.
    pub fn close(mut self) -> Result<(), Error> {
        self.workspace.remove_all(DIR)?;
        // A failed removal leaves the drop to try again.
        self.removed = true;
        Ok(())
    }
}

impl Drop for SessionFiles<'_> {
    fn drop(&mut self) {
        if !self.removed {
            // Nothing to report to: `close` is the call that returns the failure.
            let _ = self.workspace.remove_all(DIR);
        }
    }
}

/// The oldest last-record time a session may have.
fn cutoff(now: OffsetDateTime, days: u32) -> Result<String, Error> {
    now.checked_sub(time::Duration::days(i64::from(days)))
        .and_then(|time| time.to_offset(time::UtcOffset::UTC).format(&Rfc3339).ok())
        .ok_or(Error::Io(std::io::ErrorKind::InvalidInput))
}

/// A name part of a file: plain characters only, and short.
fn plain(part: &str) -> String {
    let clean: String = part
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(48)
        .collect();
    if clean.is_empty() {
        "_".to_owned()
    } else {
        clean
    }
}

fn folder_of(record: &Record) -> Option<&str> {
    match record {
        Record::Turn(t) if !t.folder.is_empty() => Some(&t.folder),
        Record::Mark(m) if !m.folder.is_empty() => Some(&m.folder),
        _ => None,
    }
}

fn branch_of(record: &Record) -> Option<&str> {
    match record {
        Record::Turn(t) if !t.branch.is_empty() => Some(&t.branch),
        Record::Mark(m) if !m.branch.is_empty() => Some(&m.branch),
        _ => None,
    }
}

/// What the drop rules need to remember between sessions of one run.
#[derive(Default)]
struct Cleaner {
    /// The opening and the length of each long user turn seen so far: two turns that begin the
    /// same but differ in length carry different payloads, so both stay.
    seen: HashSet<(String, usize)>,
}

impl Cleaner {
    fn clean(&mut self, role: Role, text: &str) -> String {
        let length = text.chars().count();
        if role == Role::User && length > LONG_TURN {
            let opening: String = text.chars().take(OPENING).collect();
            if !self.seen.insert((opening, length)) {
                return format!("[repeated instructions left out: {length} characters]");
            }
        }
        mask_secrets(&text.replace('\0', ""))
    }
}

/// The text of one session; `None` when the store has nothing for it.
fn render(tool: &str, session: &str, records: &[Record], cleaner: &mut Cleaner) -> Option<String> {
    let first = records
        .iter()
        .map(Record::time)
        .min_by(|a, b| compare_times(a, b))?;
    let last = records
        .iter()
        .map(Record::time)
        .max_by(|a, b| compare_times(a, b))?;
    let folder = records.iter().find_map(folder_of).unwrap_or("");
    let branch = records.iter().find_map(branch_of).unwrap_or("");

    let mut ordered: Vec<&Record> = records.iter().collect();
    ordered.sort_by(|a, b| compare_times(a.time(), b.time()));

    let mut out = String::new();
    // Writing to a `String` cannot fail.
    let _ = write!(
        out,
        "tool: {tool}\nsession: {}\nfolder: {folder}\nbranch: {branch}\nfirst: {first}\nlast: {last}\n",
        one_line(session)
    );
    for record in ordered {
        match record {
            Record::Turn(t) => {
                let role = match t.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                };
                let author = match (t.role, t.author) {
                    (Role::Assistant, _) | (_, Author::Unknown) => "",
                    (_, Author::Person) => " (typed by a person)",
                    (_, Author::Unattended) => " (unattended)",
                };
                let _ = write!(
                    out,
                    "\n[{}] {role}{author}:\n{}\n",
                    t.time,
                    cleaner.clean(t.role, &t.text)
                );
            }
            Record::Mark(m) if !m.tools.is_empty() => {
                let _ = write!(
                    out,
                    "\n[{}] tools used: {}\n",
                    minute_of(&m.time),
                    m.tools.join(", ")
                );
            }
            // Marks without tools say only that the session was active; the calls and results
            // of a chat are tool data, which the files leave out.
            Record::Mark(_) | Record::ToolCall(_) | Record::ToolResult(_) => {}
        }
    }
    // The workspace refuses a NUL byte, and one in a turn must not fail the whole run.
    Some(out.replace('\0', ""))
}

fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

/// The rules of [`mask_secrets`], each with what replaces its match.
#[allow(clippy::expect_used)] // The patterns are constants, so a failure is a bug found by the tests.
static RULES: LazyLock<Vec<(Regex, String)>> = LazyLock::new(|| {
    let key = r"token|secret|password|passwd|api[_-]?key|authorization";
    [
        (
            r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?(?:-----END [A-Z ]*PRIVATE KEY-----|\z)"
                .to_owned(),
            "[private key removed]".to_owned(),
        ),
        (
            r"(?i)\b(bearer\s+)[A-Za-z0-9._~+/=-]{6,}".to_owned(),
            format!("${{1}}{MASK}"),
        ),
        (
            // The whole value of an authorization header: its scheme and every part.
            r#"(?i)(authorization["']?\s*[:=]\s*)("[^"]*"|'[^']*'|[^\r\n}]*)"#.to_owned(),
            format!("${{1}}{MASK}"),
        ),
        (
            format!(
                r#"(?i)([\w.-]*(?:{key})[\w.-]*["']?\s*[:=]\s*)("[^"]*"|'[^']*'|[^\s&"',;]+)"#
            ),
            format!("${{1}}{MASK}"),
        ),
        (
            format!(r"(?i)(--?[\w-]*(?:{key})[\w-]*\s+)[^\s-]\S*"),
            format!("${{1}}{MASK}"),
        ),
        (
            r"\b(?:sk-|ghp_|gho_|ghs_|ghu_|github_pat_|glpat-|xox[abp]-|AKIA|ASIA|AIza|eyJ)[A-Za-z0-9_.-]{12,}"
                .to_owned(),
            MASK.to_owned(),
        ),
    ]
    .into_iter()
    .map(|(pattern, with)| (Regex::new(&pattern).expect("a constant pattern"), with))
    .collect()
});

/// Hides what looks like a secret: a private key block, the word after `Bearer`, the value of a
/// name such as `token`, `password` or `api_key` (after `=` or `:`, quoted or not, also in a web
/// address, or the word after a flag of that name), a well-known token shape, and the user name
/// and password of a web address. A loose rule that may hide too much: the files are never kept,
/// and a hidden word costs less than a leaked secret.
pub(crate) fn mask_secrets(text: &str) -> String {
    let mut text = strip_userinfo(text);
    for (rule, with) in RULES.iter() {
        text = rule.replace_all(&text, with.as_str()).into_owned();
    }
    text
}

/// Web addresses without the user name and password that stand before their host.
fn strip_userinfo(text: &str) -> String {
    static USERINFO: LazyLock<Regex> = LazyLock::new(|| {
        #[allow(clippy::expect_used)] // A constant pattern.
        Regex::new(r"://[^\s@]+@").expect("a constant pattern")
    });
    USERINFO.replace_all(text, "://").into_owned()
}

/// The remotes named in the git config of a folder, in the order the file has them; none when
/// the folder is gone, is not a repository or its config cannot be read. Only a file is read.
fn remotes_of(folder: &Path) -> Vec<Remote> {
    if !folder.is_absolute() {
        return Vec::new();
    }
    config_path(folder)
        .and_then(|path| read_limited(&path))
        .map(|text| parse_remotes(&text))
        .unwrap_or_default()
}

/// The config of a repository. A linked worktree has a `.git` file that points at its own
/// folder, which in turn points at the folder that holds the shared config.
fn config_path(folder: &Path) -> Option<PathBuf> {
    let dot_git = folder.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git.join("config"));
    }
    let pointer = read_limited(&dot_git)?;
    let target = pointer.trim().strip_prefix("gitdir:")?.trim();
    let git_dir = folder.join(target);
    match read_limited(&git_dir.join("commondir")) {
        Some(common) => Some(git_dir.join(common.trim()).join("config")),
        None => Some(git_dir.join("config")),
    }
}

fn read_limited(path: &Path) -> Option<String> {
    // Not blocking, and only a regular file: a named pipe in the place of a git file must not
    // hold the run.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    file.take(MAX_CONFIG).read_to_string(&mut text).ok()?;
    Some(text)
}

fn parse_remotes(config: &str) -> Vec<Remote> {
    let mut remotes = Vec::new();
    let mut current: Option<String> = None;
    for line in config.lines() {
        let line = line.trim();
        if let Some(header) = line.strip_prefix('[') {
            current = header
                .strip_prefix("remote \"")
                .and_then(|h| h.split_once("\"]"))
                .map(|(name, _)| name.to_owned());
        } else if let Some(name) = &current
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("url")
        {
            remotes.push(Remote {
                name: name.clone(),
                url: strip_userinfo(value.trim().trim_matches('"')),
            });
        }
    }
    remotes
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    use connectors::{Mark, Turn};

    use crate::records::Sink;

    use super::*;

    static NEXT: AtomicU32 = AtomicU32::new(0);

    /// A folder that removes itself.
    struct Temp(PathBuf);

    impl std::ops::Deref for Temp {
        type Target = Path;

        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn temp() -> Temp {
        let dir = std::env::temp_dir().join(format!(
            "session-files-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Temp(dir)
    }

    /// The files of a run that nothing stops.
    fn write_all<'a>(
        workspace: &'a Workspace,
        sink: &FileSink,
        sources: &[&str],
        days: Option<NonZeroU32>,
        now: OffsetDateTime,
    ) -> Result<SessionFiles<'a>, Error> {
        SessionFiles::write(workspace, sink, sources, days, now, &mut |_, _| true)
            .map(|run| run.unwrap())
    }

    fn now() -> OffsetDateTime {
        OffsetDateTime::parse("2026-11-17T12:00:00Z", &Rfc3339).unwrap()
    }

    fn turn(session: &str, time: &str, role: Role, folder: &str, text: &str) -> Record {
        Record::Turn(Turn {
            tool: "claude-code".into(),
            session: session.into(),
            time: time.into(),
            folder: folder.into(),
            branch: "feature".into(),
            role,
            author: Author::Person,
            text: text.into(),
            tools: Vec::new(),
        })
    }

    fn mark(session: &str, time: &str, tools: &[&str]) -> Record {
        Record::Mark(Mark {
            tool: "claude-code".into(),
            session: session.into(),
            time: time.into(),
            folder: "/gone".into(),
            branch: "feature".into(),
            tools: tools.iter().map(|t| (*t).to_owned()).collect(),
        })
    }

    struct Stand {
        data: Temp,
        sink: FileSink,
        workspace: Workspace,
    }

    impl Stand {
        fn new() -> Self {
            let data = temp();
            Self {
                sink: FileSink::new(&data),
                workspace: Workspace::open(&data).unwrap(),
                data,
            }
        }

        fn add(&mut self, record: &Record) {
            self.sink.append(record).unwrap();
        }

        fn run(&self, days: u32) -> SessionFiles<'_> {
            write_all(
                &self.workspace,
                &self.sink,
                &["claude-code"],
                NonZeroU32::new(days),
                now(),
            )
            .unwrap()
        }

        fn read(&self, path: &str) -> String {
            self.workspace.read(path, None, None).unwrap().join("\n")
        }
    }

    #[test]
    fn writes_a_file_per_session_with_a_header_and_the_turns_in_order() {
        let mut stand = Stand::new();
        stand.add(&turn(
            "a",
            "2026-11-10T09:00:00Z",
            Role::User,
            "/gone",
            "first question",
        ));
        stand.add(&turn(
            "a",
            "2026-11-10T09:00:30Z",
            Role::Assistant,
            "/gone",
            "the answer",
        ));
        stand.add(&turn(
            "a",
            "2026-11-10T09:05:00Z",
            Role::User,
            "/gone",
            "second question",
        ));
        stand.add(&turn(
            "b",
            "2026-11-12T08:00:00Z",
            Role::User,
            "/gone",
            "other session",
        ));
        // Written last, said between the first two.
        stand.add(&turn(
            "a",
            "2026-11-10T09:00:10Z",
            Role::User,
            "/gone",
            "said late",
        ));

        let run = stand.run(30);

        assert_eq!(run.files().len(), 2);
        // Oldest session first.
        assert!(run.files()[1].ends_with("-claude-code-b.txt"));
        let text = stand.read(&run.files()[0]);
        assert!(text.starts_with(
            "tool: claude-code\nsession: a\nfolder: /gone\nbranch: feature\n\
             first: 2026-11-10T09:00:00Z\nlast: 2026-11-10T09:05:00Z\n"
        ));
        let at = |needle: &str| text.find(needle).unwrap();
        assert!(at("first question") < at("said late"));
        assert!(at("said late") < at("the answer"));
        assert!(at("the answer") < at("second question"));
        assert!(text.contains("[2026-11-10T09:00:30Z] assistant:\nthe answer"));
        assert!(text.contains("[2026-11-10T09:00:00Z] user (typed by a person):\nfirst question"));
        assert!(!text.contains("other session"));
    }

    #[test]
    fn shows_the_tools_used_in_each_minute() {
        let mut stand = Stand::new();
        stand.add(&turn(
            "a",
            "2026-11-10T09:00:00Z",
            Role::User,
            "/gone",
            "go",
        ));
        stand.add(&mark("a", "2026-11-10T09:01:10Z", &["Read", "bash:gh"]));
        stand.add(&mark("a", "2026-11-10T09:01:50Z", &["Grep"]));
        stand.add(&mark("a", "2026-11-10T09:02:00Z", &[]));

        let run = stand.run(30);
        let text = stand.read(&run.files()[0]);

        assert!(text.contains("[2026-11-10T09:01] tools used: Read, bash:gh, Grep"));
        assert_eq!(text.matches("tools used").count(), 1);
    }

    #[test]
    fn lists_the_remotes_of_a_repository_and_none_for_a_folder_that_is_gone() {
        let mut stand = Stand::new();
        let repo = temp().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(
            repo.join(".git/config"),
            "[core]\n\turl = not-a-remote\n[remote \"origin\"]\n\turl = git@example.org:team/app.git\n\
             \tfetch = +refs/heads/*:refs/remotes/origin/*\n\
             [remote \"mirror\"]\n\turl = https://user:hunter/2@example.org/team/app.git\n\
             [branch \"main\"]\n\tremote = origin\n",
        )
        .unwrap();
        let repo = repo.to_str().unwrap();
        stand.add(&turn(
            "a",
            "2026-11-10T09:00:00Z",
            Role::User,
            repo,
            "in the repo",
        ));
        stand.add(&turn(
            "a",
            "2026-11-10T09:01:00Z",
            Role::User,
            repo,
            "again",
        ));
        stand.add(&turn(
            "b",
            "2026-11-11T09:00:00Z",
            Role::User,
            "/no/such/folder",
            "gone",
        ));

        let _run = stand.run(30);
        let json: serde_json::Value = serde_json::from_str(&stand.read(FOLDERS_FILE)).unwrap();

        let folders = json["folders"].as_array().unwrap();
        assert_eq!(folders.len(), 2);
        let found = folders.iter().find(|f| f["folder"] == repo).unwrap();
        assert_eq!(
            found["remotes"],
            serde_json::json!([
                {"name": "origin", "url": "git@example.org:team/app.git"},
                {"name": "mirror", "url": "https://example.org/team/app.git"},
            ])
        );
        let gone = folders
            .iter()
            .find(|f| f["folder"] == "/no/such/folder")
            .unwrap();
        assert_eq!(gone["remotes"], serde_json::json!([]));
    }

    #[test]
    fn follows_a_linked_worktree_to_the_shared_config() {
        let base = temp();
        let main = base.join("main");
        let tree = base.join("tree");
        fs::create_dir_all(main.join(".git/worktrees/tree")).unwrap();
        fs::create_dir_all(&tree).unwrap();
        fs::write(
            main.join(".git/config"),
            "[remote \"origin\"]\n\turl = git@example.org:team/app.git\n",
        )
        .unwrap();
        fs::write(main.join(".git/worktrees/tree/commondir"), "../..\n").unwrap();
        fs::write(
            tree.join(".git"),
            format!("gitdir: {}\n", main.join(".git/worktrees/tree").display()),
        )
        .unwrap();

        let remotes = remotes_of(&tree);

        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0].url, "git@example.org:team/app.git");
        assert!(remotes_of(Path::new("relative/folder")).is_empty());
    }

    #[test]
    fn reads_a_session_that_is_still_being_written_as_it_is_then() {
        let mut stand = Stand::new();
        stand.add(&turn(
            "a",
            "2026-11-17T10:00:00Z",
            Role::User,
            "/gone",
            "before",
        ));

        let first = stand.run(30);
        let early = stand.read(&first.files()[0]);
        drop(first);

        stand.add(&turn(
            "a",
            "2026-11-17T10:01:00Z",
            Role::Assistant,
            "/gone",
            "after",
        ));
        // A line the writer has not finished yet.
        let path = stand.sink.session_files().unwrap().remove(0);
        let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
        std::io::Write::write_all(&mut file, b"{\"kind\":\"turn\",\"tool\":\"cla").unwrap();

        let second = stand.run(30);
        let late = stand.read(&second.files()[0]);

        assert!(early.contains("before") && !early.contains("after"));
        assert!(late.contains("before") && late.contains("after"));
        assert!(late.contains("last: 2026-11-17T10:01:00Z"));
    }

    #[test]
    fn removes_the_files_when_the_run_ends() {
        let mut stand = Stand::new();
        stand.add(&turn(
            "a",
            "2026-11-10T09:00:00Z",
            Role::User,
            "/gone",
            "text",
        ));

        let run = stand.run(30);
        assert!(stand.data.join("workspace/history").is_dir());
        run.close().unwrap();
        assert!(!stand.data.join("workspace/history").exists());

        let run = stand.run(30);
        assert!(stand.data.join("workspace/history").is_dir());
        drop(run);
        assert!(!stand.data.join("workspace/history").exists());
    }

    #[test]
    fn replaces_what_an_earlier_run_left() {
        let mut stand = Stand::new();
        stand.add(&turn(
            "a",
            "2026-11-10T09:00:00Z",
            Role::User,
            "/gone",
            "text",
        ));
        stand
            .workspace
            .write("history/left-over.txt", "old")
            .unwrap();
        stand.workspace.write("notes.txt", "keep").unwrap();

        let run = stand.run(30);

        assert!(
            stand
                .workspace
                .read("history/left-over.txt", None, None)
                .is_err()
        );
        assert_eq!(stand.read("notes.txt"), "keep");
        drop(run);
    }

    #[test]
    fn the_period_limits_the_sessions_and_their_folders() {
        let mut stand = Stand::new();
        stand.add(&turn(
            "old",
            "2026-10-01T09:00:00Z",
            Role::User,
            "/old",
            "long ago",
        ));
        stand.add(&turn(
            "recent",
            "2026-11-14T09:00:00Z",
            Role::User,
            "/new",
            "lately",
        ));

        assert_eq!(stand.run(60).files().len(), 2);
        assert_eq!(stand.run(1).files().len(), 0);
        let run = stand.run(30);
        assert_eq!(run.files().len(), 1);
        assert!(stand.read(&run.files()[0]).contains("lately"));
        let json: serde_json::Value = serde_json::from_str(&stand.read(FOLDERS_FILE)).unwrap();
        assert_eq!(json["folders"].as_array().unwrap().len(), 1);
        assert_eq!(json["folders"][0]["folder"], "/new");
        drop(run);

        // No period given: the default of 30 days.
        let default =
            write_all(&stand.workspace, &stand.sink, &["claude-code"], None, now()).unwrap();
        assert_eq!(default.files().len(), 1);
    }

    #[test]
    fn keeps_one_copy_of_each_repeated_block_and_leaves_tool_data_out() {
        let mut stand = Stand::new();
        let block = |opening: &str| format!("{opening} {}", "Follow the rules. ".repeat(200));
        let first = block("You are the review bot.");
        let other = block("You are the test bot.");
        stand.add(&turn(
            "a",
            "2026-11-10T09:00:00Z",
            Role::User,
            "/gone",
            &first,
        ));
        stand.add(&turn(
            "b",
            "2026-11-11T09:00:00Z",
            Role::User,
            "/gone",
            &first,
        ));
        stand.add(&turn(
            "b",
            "2026-11-11T09:01:00Z",
            Role::User,
            "/gone",
            &other,
        ));
        stand.add(&turn(
            "c",
            "2026-11-12T09:00:00Z",
            Role::User,
            "/gone",
            &first,
        ));
        stand.add(&turn(
            "c",
            "2026-11-12T09:01:00Z",
            Role::User,
            "/gone",
            "a short one",
        ));
        stand.add(&Record::ToolCall(connectors::ToolCall {
            tool: "claude-code".into(),
            session: "b".into(),
            time: "2026-11-11T09:02:00Z".into(),
            call_id: "c1".into(),
            name: "Read".into(),
            arguments: "{\"path\":\"/etc/passwd\"}".into(),
        }));

        let run = stand.run(30);
        let files: Vec<String> = run.files().iter().map(|f| stand.read(f)).collect();

        assert!(files[0].contains("You are the review bot."));
        // The second copy goes, a different block and a short turn stay.
        assert!(!files[1].contains("You are the review bot."));
        assert!(files[1].contains("[repeated instructions left out:"));
        assert!(files[1].contains("You are the test bot."));
        assert!(!files[2].contains("You are the review bot."));
        assert!(files[2].contains("a short one"));
        assert!(!files[1].contains("/etc/passwd"));
    }

    #[test]
    fn hides_secrets_in_every_shape_and_keeps_the_text_around() {
        let mut stand = Stand::new();
        stand.add(&turn(
            "a",
            "2026-11-10T09:00:00Z",
            Role::Assistant,
            "/gone",
            "see https://host/x?api_key=q1&access_token=q2 then password: q3 and password = q4\n\
             run tool --token q5 with FOO=ghp_0123456789abcdefghij and \"password\": \"q6\"\n\
             {\"Authorization\": \"Bearer q7q7q7q7\"} or https://me:p/w@example.org/x.git\n\
             Authorization: Basic q9q9q9q9\nAuthorization: Digest username=\"q10\", response=\"q11\"\n\
             -----BEGIN PRIVATE KEY-----\nq8\n-----END PRIVATE KEY-----\nthe end",
        ));

        let run = stand.run(30);
        let text = stand.read(&run.files()[0]);

        for secret in [
            "q1", "q2", "q3", "q4", "q5", "ghp_0123", "q6", "q7q7", "q8", "p/w@", "q9q9", "q10",
            "q11",
        ] {
            assert!(!text.contains(secret), "{secret} is still there");
        }
        assert!(text.contains("see https://host/x?api_key=[secret removed]"));
        assert!(text.contains("https://example.org/x.git"));
        assert!(text.contains("the end"));
    }

    #[test]
    fn long_turns_with_one_opening_and_different_lengths_both_stay() {
        let mut stand = Stand::new();
        let long = |tail: &str| format!("Fixed instructions. {}{tail}", "Rule. ".repeat(500));
        stand.add(&turn(
            "a",
            "2026-11-10T09:00:00Z",
            Role::User,
            "/gone",
            &long("payload one"),
        ));
        stand.add(&turn(
            "a",
            "2026-11-10T09:01:00Z",
            Role::User,
            "/gone",
            &long("a longer payload two"),
        ));

        let run = stand.run(30);
        let text = stand.read(&run.files()[0]);

        assert!(text.contains("payload one") && text.contains("a longer payload two"));
    }

    #[test]
    fn a_run_that_is_stopped_leaves_no_files() {
        let mut stand = Stand::new();
        for (session, time) in [("a", "2026-11-10T09:00:00Z"), ("b", "2026-11-11T09:00:00Z")] {
            stand.add(&turn(session, time, Role::User, "/gone", "a question"));
        }
        let mut asked = Vec::new();
        let run = SessionFiles::write(
            &stand.workspace,
            &stand.sink,
            &["claude-code"],
            None,
            now(),
            // Goes on for the first session, stops before the second.
            &mut |sessions, _| {
                asked.push(sessions);
                sessions < 1
            },
        )
        .unwrap();
        assert!(run.is_none());
        assert_eq!(asked, [0, 1]);
        assert!(!stand.data.join("workspace/history").exists());
    }

    #[test]
    fn a_period_out_of_the_range_of_dates_is_an_error() {
        let stand = Stand::new();

        let result = write_all(
            &stand.workspace,
            &stand.sink,
            &["claude-code"],
            NonZeroU32::new(u32::MAX),
            OffsetDateTime::from_unix_timestamp(0).unwrap(),
        );

        assert!(result.is_err());
        assert!(!stand.data.join("workspace/history").exists());
    }

    #[test]
    fn a_nul_character_in_a_turn_does_not_fail_the_run() {
        let mut stand = Stand::new();
        stand.add(&turn(
            "a",
            "2026-11-10T09:00:00Z",
            Role::User,
            "/gone",
            "bad\0text and tok\0en=sekret99",
        ));

        let run = stand.run(30);

        let text = stand.read(&run.files()[0]);
        assert!(text.contains("badtext"));
        assert!(!text.contains("sekret99"));
    }

    #[test]
    fn replaces_a_file_that_stands_where_the_folder_goes() {
        let mut stand = Stand::new();
        stand.add(&turn(
            "a",
            "2026-11-10T09:00:00Z",
            Role::User,
            "/gone",
            "text",
        ));
        stand.workspace.write(DIR, "not a folder").unwrap();

        let run = stand.run(30);

        assert_eq!(run.files().len(), 1);
    }

    #[test]
    fn a_folder_that_is_not_a_repository_has_no_remotes() {
        let mut stand = Stand::new();
        let plain_folder = temp();
        let folder = plain_folder.to_str().unwrap();
        stand.add(&turn(
            "a",
            "2026-11-10T09:00:00Z",
            Role::User,
            folder,
            "text",
        ));

        let _run = stand.run(30);
        let json: serde_json::Value = serde_json::from_str(&stand.read(FOLDERS_FILE)).unwrap();

        assert_eq!(json["folders"][0]["remotes"], serde_json::json!([]));
    }

    #[test]
    fn a_named_pipe_in_place_of_a_git_file_does_not_hold_the_run() {
        let folder = temp();
        let status = std::process::Command::new("mkfifo")
            .arg(folder.join(".git"))
            .status()
            .unwrap();
        assert!(status.success());

        assert!(remotes_of(&folder).is_empty());
    }

    #[test]
    fn keeps_ordinary_words_that_only_look_like_secrets() {
        assert_eq!(
            mask_secrets("a sk-learn model, the token count: 5\nnext  line"),
            "a sk-learn model, the token count: 5\nnext  line"
        );
    }

    #[test]
    fn files_are_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;

        let mut stand = Stand::new();
        stand.add(&turn(
            "a",
            "2026-11-10T09:00:00Z",
            Role::User,
            "/gone",
            "text",
        ));

        let run = stand.run(30);
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;

        assert_eq!(mode(&stand.data.join("workspace/history")), 0o700);
        let file = stand.data.join("workspace").join(&run.files()[0]);
        assert_eq!(mode(&file), 0o600);
    }
}
