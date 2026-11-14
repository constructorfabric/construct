use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::text::{join_texts, root_under};
use crate::{FileState, Listing, Mark, Parsed, Record, Role, Tool, Turn};

/// The tool's own context that Codex writes as user messages. A text block that starts
/// with one of these is not something the user typed.
const CONTEXT_MARKERS: [&str; 11] = [
    "<environment_context>",
    "<recommended_plugins>",
    "<user_instructions>",
    "<INSTRUCTIONS>",
    "<permissions instructions>",
    "<collaboration_mode>",
    "<skills_instructions>",
    "<turn_aborted>",
    "<user_shell_command>",
    "<subagent_notification>",
    "# AGENTS.md instructions",
];

/// Codex: `<home>/sessions/YYYY/MM/DD/rollout-<time>-<uuid>.jsonl`, where `<home>` is
/// `$CODEX_HOME` or `~/.codex`.
///
/// The folder and branch are written once, at the start of a file; the session id is also
/// in the file name. All three are kept in the [`FileState`], so a read that starts in the
/// middle of a file knows them.
pub struct Codex;

impl Codex {
    fn root_in(home: &Path, codex_home: Option<OsString>) -> PathBuf {
        root_under(home, codex_home, ".codex", "sessions")
    }
}

/// The text blocks of a message of `role`, one after the other, without the tool's own
/// context in user messages; `None` when nothing is left.
fn join_text(role: Role, blocks: &[Block]) -> Option<String> {
    let wanted = match role {
        Role::User => "input_text",
        Role::Assistant => "output_text",
    };
    join_texts(
        blocks
            .iter()
            .filter(|b| b.kind == wanted)
            .filter_map(|b| b.text.as_deref())
            .filter(|t| role == Role::Assistant || !is_context(t)),
    )
}

fn is_context(text: &str) -> bool {
    let text = text.trim_start();
    CONTEXT_MARKERS.iter().any(|m| text.starts_with(m))
}

/// The id of a session from a file name `rollout-<time>-<uuid>.jsonl`.
fn session_of(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?.strip_prefix("rollout-")?;
    let id = stem.get(stem.len().checked_sub(36)?..)?;
    let shaped = id.bytes().enumerate().all(|(i, b)| {
        if matches!(i, 8 | 13 | 18 | 23) {
            b == b'-'
        } else {
            b.is_ascii_hexdigit()
        }
    });
    shaped.then(|| id.to_owned())
}

fn collect_files(dir: &Path, depth: u8, listing: &mut Listing) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(e) => {
                listing
                    .problems
                    .push(format!("cannot list {}: {e}", dir.display()));
                continue;
            }
        };
        if path.is_dir() {
            if depth < 3 {
                // A day directory that cannot be read is named; the other days are still read.
                if let Err(e) = collect_files(&path, depth + 1, listing) {
                    listing
                        .problems
                        .push(format!("cannot list {}: {e}", path.display()));
                }
            }
        } else if path.is_file()
            && path.extension().is_some_and(|e| e == "jsonl")
            && path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("rollout-"))
        {
            listing.files.push(path);
        }
    }
    Ok(())
}

impl Tool for Codex {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn root(&self, home: &Path) -> PathBuf {
        Self::root_in(home, env::var_os("CODEX_HOME"))
    }

    fn find_files(&self, root: &Path) -> io::Result<Listing> {
        let mut listing = Listing::default();
        collect_files(root, 0, &mut listing)?;
        listing.files.sort();
        Ok(listing)
    }

    fn open_file(&self, path: &Path) -> FileState {
        FileState {
            session: session_of(path).unwrap_or_default(),
            ..FileState::default()
        }
    }

    fn parse_line(&self, state: &mut FileState, line: &str) -> Parsed {
        let Ok(line) = serde_json::from_str::<Line>(line) else {
            return Parsed::Skipped;
        };
        let payload = line.payload.unwrap_or_default();
        match line.kind.as_str() {
            "session_meta" => {
                if state.session.is_empty() {
                    state.session = payload.id.unwrap_or_default();
                }
                if let Some(cwd) = payload.cwd.filter(|c| !c.is_empty()) {
                    state.folder = cwd;
                }
                if let Some(branch) = payload.git.and_then(|g| g.branch) {
                    state.branch = branch;
                }
                return Parsed::Ignored;
            }
            "turn_context" => {
                if let Some(cwd) = payload.cwd.filter(|c| !c.is_empty()) {
                    state.folder = cwd;
                }
                return Parsed::Ignored;
            }
            "response_item" | "event_msg" => {}
            _ => return Parsed::Ignored,
        }
        let Some(time) = line.timestamp else {
            return Parsed::Ignored;
        };
        let role = match (
            line.kind.as_str(),
            payload.kind.as_deref(),
            payload.role.as_deref(),
        ) {
            ("response_item", Some("message"), Some("user")) => Some(Role::User),
            ("response_item", Some("message"), Some("assistant")) => Some(Role::Assistant),
            ("response_item", Some("message"), Some("developer")) => return Parsed::Ignored,
            _ => None,
        };
        let text = role.and_then(|r| join_text(r, &payload.content.unwrap_or_default()));
        match (role, text) {
            (Some(role), Some(text)) => Parsed::Record(Record::Turn(Turn {
                tool: self.name().into(),
                session: state.session.clone(),
                time,
                folder: state.folder.clone(),
                branch: state.branch.clone(),
                role,
                text,
            })),
            _ => Parsed::Record(Record::Mark(Mark {
                tool: self.name().into(),
                session: state.session.clone(),
                time,
                folder: state.folder.clone(),
                branch: state.branch.clone(),
            })),
        }
    }
}

/// The few fields of a line that are read; the rest (instructions, tool outputs, ...) is
/// skipped while parsing and never held in memory.
#[derive(Deserialize)]
struct Line {
    timestamp: Option<String>,
    #[serde(rename = "type", default)]
    kind: String,
    payload: Option<Payload>,
}

#[derive(Default, Deserialize)]
struct Payload {
    #[serde(rename = "type")]
    kind: Option<String>,
    id: Option<String>,
    cwd: Option<String>,
    git: Option<Git>,
    role: Option<String>,
    content: Option<Vec<Block>>,
}

#[derive(Deserialize)]
struct Git {
    branch: Option<String>,
}

#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type", default)]
    kind: String,
    text: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "01a0bee2-6260-73a3-b389-43c86bb8930f";
    const T: &str = "2026-11-11T10:15:30.123Z";

    fn path() -> PathBuf {
        PathBuf::from(format!(
            "/h/.codex/sessions/2026/11/11/rollout-2026-11-11T10-15-00-{ID}.jsonl"
        ))
    }

    fn item(kind: &str, payload: &str) -> String {
        format!(r#"{{"timestamp":"{T}","type":"{kind}","payload":{payload}}}"#)
    }

    fn message(role: &str, block: &str, text: &str) -> String {
        item(
            "response_item",
            &format!(
                r#"{{"type":"message","role":"{role}","content":[{{"type":"{block}","text":"{text}"}}]}}"#
            ),
        )
    }

    fn meta(extra: &str) -> String {
        item(
            "session_meta",
            &format!(
                r#"{{"id":"{ID}","cwd":"/w/app","originator":"cli","base_instructions":{{"text":"long"}}{extra}}}"#
            ),
        )
    }

    /// A state after the first lines of a file were read.
    fn started(meta_line: &str) -> FileState {
        let mut st = Codex.open_file(&path());
        assert_eq!(Codex.parse_line(&mut st, meta_line), Parsed::Ignored);
        st
    }

    fn turn(p: Parsed) -> Turn {
        match p {
            Parsed::Record(Record::Turn(t)) => t,
            other => panic!("not a turn: {other:?}"),
        }
    }

    fn assert_mark(p: &Parsed, folder: &str, branch: &str) {
        let Parsed::Record(Record::Mark(m)) = p else {
            panic!("not a mark: {p:?}")
        };
        assert_eq!(
            (m.session.as_str(), m.folder.as_str(), m.branch.as_str()),
            (ID, folder, branch)
        );
        assert_eq!((m.tool.as_str(), m.time.as_str()), ("codex", T));
    }

    #[test]
    fn session_meta_with_git() {
        let st = started(&meta(r#","git":{"commit_hash":"abc","branch":"feat"}"#));
        assert_eq!(
            (st.session.as_str(), st.folder.as_str(), st.branch.as_str()),
            (ID, "/w/app", "feat")
        );
    }

    #[test]
    fn session_meta_gives_the_session_only_when_the_file_name_does_not() {
        let mut st = Codex.open_file(Path::new("/x/rollout-bad.jsonl"));
        assert_eq!(Codex.parse_line(&mut st, &meta("")), Parsed::Ignored);
        assert_eq!(st.session, ID);
        let other = meta("").replace(ID, "ffffffff-0000-0000-0000-000000000000");
        let mut named = Codex.open_file(&path());
        assert_eq!(Codex.parse_line(&mut named, &other), Parsed::Ignored);
        assert_eq!(named.session, ID);
    }

    #[test]
    fn session_meta_without_git() {
        for extra in ["", r#","git":{"commit_hash":"abc"}"#] {
            let st = started(&meta(extra));
            assert_eq!(
                (st.session.as_str(), st.folder.as_str(), st.branch.as_str()),
                (ID, "/w/app", "")
            );
        }
    }

    #[test]
    fn user_and_assistant_messages_are_turns() {
        let mut st = started(&meta(r#","git":{"branch":"feat"}"#));
        let u = turn(Codex.parse_line(&mut st, &message("user", "input_text", "fix it\\nnow")));
        assert_eq!(
            (u.role, u.text.as_str(), u.time.as_str()),
            (Role::User, "fix it\nnow", T)
        );
        assert_eq!(
            (u.session.as_str(), u.folder.as_str(), u.branch.as_str()),
            (ID, "/w/app", "feat")
        );
        let a = turn(Codex.parse_line(&mut st, &message("assistant", "output_text", "Done.")));
        assert_eq!(
            (a.role, a.text.as_str(), a.tool.as_str()),
            (Role::Assistant, "Done.", "codex")
        );
    }

    #[test]
    fn developer_message_is_nothing() {
        let mut st = started(&meta(""));
        assert_eq!(
            Codex.parse_line(&mut st, &message("developer", "input_text", "rules")),
            Parsed::Ignored
        );
    }

    #[test]
    fn the_tools_own_context_is_a_mark_only() {
        let mut st = started(&meta(""));
        for marker in CONTEXT_MARKERS {
            let text = format!("{marker}\\nrest of the block");
            let p = Codex.parse_line(&mut st, &message("user", "input_text", &text));
            assert_mark(&p, "/w/app", "");
        }
        let two = item(
            "response_item",
            r#"{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>x</environment_context>"},{"type":"input_text","text":"the real prompt"}]}"#,
        );
        assert_eq!(
            turn(Codex.parse_line(&mut st, &two)).text,
            "the real prompt"
        );
    }

    #[test]
    fn reasoning_tool_calls_and_events_are_marks_only() {
        let mut st = started(&meta(""));
        for p in [
            r#"{"type":"reasoning","summary":[],"encrypted_content":"AAAA"}"#,
            r#"{"type":"function_call","name":"shell","arguments":"{}","call_id":"c1"}"#,
            r#"{"type":"function_call_output","call_id":"c1","output":"a very long output"}"#,
        ] {
            assert_mark(
                &Codex.parse_line(&mut st, &item("response_item", p)),
                "/w/app",
                "",
            );
        }
        let ev = item("event_msg", r#"{"type":"agent_message","message":"hi"}"#);
        assert_mark(&Codex.parse_line(&mut st, &ev), "/w/app", "");
    }

    #[test]
    fn other_lines_are_ignored_and_turn_context_updates_the_folder() {
        let mut st = started(&meta(""));
        assert_eq!(
            Codex.parse_line(&mut st, &item("token_usage_record", "{}")),
            Parsed::Ignored
        );
        let tc = item("turn_context", r#"{"cwd":"/w/other","model":"m"}"#);
        assert_eq!(Codex.parse_line(&mut st, &tc), Parsed::Ignored);
        assert_eq!(st.folder, "/w/other");
    }

    #[test]
    fn unparseable_line_is_skipped() {
        let mut st = Codex.open_file(&path());
        assert_eq!(
            Codex.parse_line(&mut st, r#"{"timestamp":"2026-11-11T10:1"#),
            Parsed::Skipped
        );
        assert_eq!(Codex.parse_line(&mut st, "not json"), Parsed::Skipped);
    }

    #[test]
    fn a_read_from_the_middle_uses_the_saved_state() {
        let mut first = started(&meta(r#","git":{"branch":"feat"}"#));
        let saved = serde_json::to_string(&first).unwrap();
        assert!(matches!(
            Codex.parse_line(&mut first, &message("user", "input_text", "one")),
            Parsed::Record(_)
        ));
        // The agent restarts and reads on from its saved offset: no session_meta in sight.
        let mut restored: FileState = serde_json::from_str(&saved).unwrap();
        let t = turn(Codex.parse_line(&mut restored, &message("assistant", "output_text", "two")));
        assert_eq!(
            (t.session.as_str(), t.folder.as_str(), t.branch.as_str()),
            (ID, "/w/app", "feat")
        );
        // A file never read, entered in the middle: the id still comes from the file name.
        let mut fresh = Codex.open_file(&path());
        let t = turn(Codex.parse_line(&mut fresh, &message("user", "input_text", "three")));
        assert_eq!((t.session.as_str(), t.folder.as_str()), (ID, ""));
    }

    #[test]
    fn session_id_is_taken_from_the_file_name() {
        assert_eq!(Codex.open_file(&path()).session, ID);
        assert_eq!(
            Codex.open_file(Path::new("/x/rollout-bad.jsonl")).session,
            ""
        );
    }

    #[test]
    fn finds_rollout_files_and_honours_codex_home() {
        let dir = std::env::temp_dir().join(format!("codex-find-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let day = dir.join("sessions/2026/11/11");
        fs::create_dir_all(&day).unwrap();
        fs::write(
            day.join(format!("rollout-2026-11-11T10-15-00-{ID}.jsonl")),
            "",
        )
        .unwrap();
        fs::write(day.join("other.jsonl"), "").unwrap();
        let files = Codex.find_files(&dir.join("sessions")).unwrap().files;
        assert_eq!(files.len(), 1);
        assert!(
            Codex
                .find_files(&dir.join("missing"))
                .unwrap()
                .files
                .is_empty()
        );
        let home = Path::new("/h");
        assert_eq!(Codex::root_in(home, None), Path::new("/h/.codex/sessions"));
        assert_eq!(
            Codex::root_in(home, Some("/cx".into())),
            Path::new("/cx/sessions")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_day_directory_that_cannot_be_listed_is_named_and_the_other_days_are_still_listed() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("codex-listing-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (bad, good) = (root.join("2026/11/10"), root.join("2026/11/11"));
        fs::create_dir_all(&bad).unwrap();
        fs::create_dir_all(&good).unwrap();
        let log = good.join(format!("rollout-2026-11-11T10-15-00-{ID}.jsonl"));
        fs::write(&log, "").unwrap();
        fs::set_permissions(&bad, fs::Permissions::from_mode(0o000)).unwrap();
        let locked = fs::read_dir(&bad).is_err();
        let listing = Codex.find_files(&root).unwrap();
        fs::set_permissions(&bad, fs::Permissions::from_mode(0o755)).unwrap();
        let _ = fs::remove_dir_all(&root);
        if !locked {
            return;
        }
        assert_eq!(listing.files, [log]);
        assert_eq!(listing.problems.len(), 1);
        assert!(
            listing.problems[0].starts_with(&format!("cannot list {}", bad.display())),
            "{:?}",
            listing.problems
        );
    }
}
