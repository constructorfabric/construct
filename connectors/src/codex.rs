use std::env;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::text::{entries_of, is_jsonl_file, join_texts, root_under, starts_with_marker};
use crate::{FileState, Record, Role, Tool};

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
/// The session id, folder and branch are written once, at the start of a file. They are
/// kept in the [`FileState`], so a read that starts in the middle of a file knows them.
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
    starts_with_marker(text, &CONTEXT_MARKERS)
}

fn collect_files(dir: &Path, depth: u8, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for path in entries_of(dir)? {
        if path.is_dir() {
            if depth < 3 {
                collect_files(&path, depth + 1, files)?;
            }
        } else if is_jsonl_file(&path)
            && path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("rollout-"))
        {
            files.push(path);
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

    fn find_files(&self, root: &Path) -> io::Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        collect_files(root, 0, &mut files)?;
        files.sort();
        Ok(files)
    }

    fn read_line(&self, state: &mut FileState, line: &str) -> Option<Record> {
        let line = serde_json::from_str::<Line>(line).ok()?;
        let payload = line.payload.unwrap_or_default();
        match line.kind.as_str() {
            "session_meta" => {
                if let Some(id) = payload.id.filter(|i| !i.is_empty()) {
                    state.session = id;
                }
                if let Some(cwd) = payload.cwd.filter(|c| !c.is_empty()) {
                    state.folder = cwd;
                }
                if let Some(branch) = payload.git.and_then(|g| g.branch) {
                    state.branch = branch;
                }
                return None;
            }
            "turn_context" => {
                if let Some(cwd) = payload.cwd.filter(|c| !c.is_empty()) {
                    state.folder = cwd;
                }
                return None;
            }
            "response_item" | "event_msg" => {}
            _ => return None,
        }
        let time = line.timestamp?;
        let name = self.name();
        let role = match (
            line.kind.as_str(),
            payload.kind.as_deref(),
            payload.role.as_deref(),
        ) {
            ("response_item", Some("message"), Some("user")) => Some(Role::User),
            ("response_item", Some("message"), Some("assistant")) => Some(Role::Assistant),
            ("response_item", Some("message"), Some("developer")) => return None,
            _ => None,
        };
        let text = role.and_then(|r| join_text(r, &payload.content.unwrap_or_default()));
        Some(match (role, text) {
            (Some(role), Some(text)) => state.turn(name, time, role, text),
            _ => state.mark(name, time),
        })
    }
}

/// The few fields of a line that are read. Fields that are not listed here are skipped
/// while the line is parsed and not kept; the content of a message is parsed whole.
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
    use crate::read_new_lines;
    use crate::text::Scratch;
    use crate::{Mark, Turn};

    const ID: &str = "01a0bee2-6260-73a3-b389-43c86bb8930f";
    const T: &str = "2026-11-11T10:15:30.123Z";

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

    /// A state after the first line of a file was read.
    fn started(meta_line: &str) -> FileState {
        let mut st = FileState::default();
        assert!(Codex.read_line(&mut st, meta_line).is_none());
        st
    }

    fn turn(r: Option<Record>) -> Turn {
        match r {
            Some(Record::Turn(t)) => t,
            other => panic!("not a turn: {other:?}"),
        }
    }

    fn mark(r: Option<Record>) -> Mark {
        match r {
            Some(Record::Mark(m)) => m,
            other => panic!("not a mark: {other:?}"),
        }
    }

    fn assert_mark(r: Option<Record>, folder: &str, branch: &str) {
        let m = mark(r);
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
        let u = turn(Codex.read_line(&mut st, &message("user", "input_text", "fix it\\nnow")));
        assert_eq!(
            (u.role, u.text.as_str(), u.time.as_str()),
            (Role::User, "fix it\nnow", T)
        );
        assert_eq!(
            (u.session.as_str(), u.folder.as_str(), u.branch.as_str()),
            (ID, "/w/app", "feat")
        );
        let a = turn(Codex.read_line(&mut st, &message("assistant", "output_text", "Done.")));
        assert_eq!(
            (a.role, a.text.as_str(), a.tool.as_str()),
            (Role::Assistant, "Done.", "codex")
        );
    }

    #[test]
    fn a_block_of_another_type_is_not_text() {
        let mut st = started(&meta(""));
        assert_mark(
            Codex.read_line(&mut st, &message("user", "input_image", "AAAA")),
            "/w/app",
            "",
        );
        assert_mark(
            Codex.read_line(&mut st, &message("assistant", "input_text", "echo")),
            "/w/app",
            "",
        );
    }

    #[test]
    fn developer_message_is_nothing() {
        let mut st = started(&meta(""));
        let rules = message("developer", "input_text", "rules");
        assert!(Codex.read_line(&mut st, &rules).is_none());
    }

    #[test]
    fn the_tools_own_context_is_a_mark_only() {
        let mut st = started(&meta(""));
        for marker in CONTEXT_MARKERS {
            let text = format!("{marker}\\nrest of the block");
            let r = Codex.read_line(&mut st, &message("user", "input_text", &text));
            assert_mark(r, "/w/app", "");
        }
        let indented = message("user", "input_text", "  \\n<environment_context>x");
        assert_mark(Codex.read_line(&mut st, &indented), "/w/app", "");
        let two = item(
            "response_item",
            r#"{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>x</environment_context>"},{"type":"input_text","text":"the real prompt"}]}"#,
        );
        assert_eq!(turn(Codex.read_line(&mut st, &two)).text, "the real prompt");
    }

    #[test]
    fn a_marker_inside_the_text_or_in_an_assistant_message_is_still_a_turn() {
        let mut st = started(&meta(""));
        let inside = message(
            "user",
            "input_text",
            "why does <environment_context> show up",
        );
        assert_eq!(turn(Codex.read_line(&mut st, &inside)).role, Role::User);
        let said = message(
            "assistant",
            "output_text",
            "<environment_context> is added by Codex",
        );
        assert_eq!(turn(Codex.read_line(&mut st, &said)).role, Role::Assistant);
    }

    #[test]
    fn reasoning_tool_calls_and_events_are_marks_only() {
        let mut st = started(&meta(""));
        for p in [
            r#"{"type":"reasoning","summary":[],"encrypted_content":"AAAA"}"#,
            r#"{"type":"function_call","name":"shell","arguments":"{}","call_id":"c1"}"#,
            r#"{"type":"function_call_output","call_id":"c1","output":"a very long output"}"#,
        ] {
            let r = Codex.read_line(&mut st, &item("response_item", p));
            assert_mark(r, "/w/app", "");
        }
        let ev = item("event_msg", r#"{"type":"agent_message","message":"hi"}"#);
        assert_mark(Codex.read_line(&mut st, &ev), "/w/app", "");
    }

    #[test]
    fn other_lines_are_ignored_and_turn_context_updates_the_folder() {
        let mut st = started(&meta(""));
        assert!(
            Codex
                .read_line(&mut st, &item("token_usage_record", "{}"))
                .is_none()
        );
        let tc = item("turn_context", r#"{"cwd":"/w/other","model":"m"}"#);
        assert!(Codex.read_line(&mut st, &tc).is_none());
        assert_eq!(st.folder, "/w/other");
    }

    #[test]
    fn a_line_without_a_time_is_ignored() {
        let mut st = started(&meta(""));
        let l = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}}"#;
        assert!(Codex.read_line(&mut st, l).is_none());
    }

    #[test]
    fn unparseable_line_is_ignored() {
        let mut st = FileState::default();
        assert!(
            Codex
                .read_line(&mut st, r#"{"timestamp":"2026-11-11T10:1"#)
                .is_none()
        );
        assert!(Codex.read_line(&mut st, "not json").is_none());
    }

    #[test]
    fn session_folder_and_branch_stay_in_the_state_for_later_lines() {
        let mut st = started(&meta(r#","git":{"branch":"feat"}"#));
        let saved = serde_json::to_string(&st).unwrap();
        let t = turn(Codex.read_line(&mut st, &message("user", "input_text", "one")));
        assert_eq!(
            (t.session.as_str(), t.folder.as_str(), t.branch.as_str()),
            (ID, "/w/app", "feat")
        );
        // A restart reads on from a saved offset: the saved state stands in for session_meta.
        let mut restored: FileState = serde_json::from_str(&saved).unwrap();
        let t = turn(Codex.read_line(&mut restored, &message("assistant", "output_text", "two")));
        assert_eq!(
            (t.session.as_str(), t.folder.as_str(), t.branch.as_str()),
            (ID, "/w/app", "feat")
        );
    }

    #[test]
    fn finds_rollout_files_in_order_and_honours_codex_home() {
        let dir = Scratch::new("codex-find");
        let day = |name: &str| format!("sessions/2026/11/11/{name}");
        dir.write(&day("rollout-2026-11-11T11-00-00-b.jsonl"), "");
        dir.write(&day("rollout-2026-11-11T10-00-00-a.jsonl"), "");
        dir.write(&day("other.jsonl"), "");
        dir.write(&day("rollout-notes.txt"), "");
        dir.write("sessions/rollout-too-high.jsonl", "");
        dir.write("sessions/2026/11/11/12/too/deep/rollout-x.jsonl", "");
        let root = dir.path().join("sessions");
        let files = Codex.find_files(&root).unwrap();
        assert_eq!(
            files,
            [
                root.join("2026/11/11/rollout-2026-11-11T10-00-00-a.jsonl"),
                root.join("2026/11/11/rollout-2026-11-11T11-00-00-b.jsonl"),
                root.join("rollout-too-high.jsonl"),
            ]
        );
        assert!(
            Codex
                .find_files(&dir.path().join("none"))
                .unwrap()
                .is_empty()
        );
        let home = Path::new("/h");
        assert_eq!(Codex::root_in(home, None), Path::new("/h/.codex/sessions"));
        assert_eq!(
            Codex::root_in(home, Some("/cx".into())),
            Path::new("/cx/sessions")
        );
    }

    #[test]
    fn a_session_file_is_read_in_two_steps_from_the_saved_offset() {
        let dir = Scratch::new("codex-read");
        let first = [
            meta(r#","git":{"branch":"feat"}"#),
            message("user", "input_text", "fix the build"),
            "{damaged".to_owned(),
            message("assistant", "output_text", "Done."),
        ];
        let name = format!("sessions/2026/11/11/rollout-2026-11-11T10-15-00-{ID}.jsonl");
        let path = dir.write(&name, &(first.join("\n") + "\n"));
        let files = Codex.find_files(&dir.path().join("sessions")).unwrap();
        assert_eq!(files, std::slice::from_ref(&path));

        let mut state = FileState::default();
        let (lines, offset) = read_new_lines(&path, 0).unwrap();
        let records: Vec<Record> = lines
            .iter()
            .filter_map(|l| Codex.read_line(&mut state, l))
            .collect();
        let texts: Vec<&str> = records
            .iter()
            .filter_map(|r| match r {
                Record::Turn(t) => Some(t.text.as_str()),
                Record::Mark(_) => None,
            })
            .collect();
        assert_eq!(texts, ["fix the build", "Done."]);

        let more = message("user", "input_text", "and the docs");
        std::fs::write(&path, first.join("\n") + "\n" + &more + "\n").unwrap();
        let (lines, _) = read_new_lines(&path, offset).unwrap();
        assert_eq!(lines.len(), 1);
        let t = turn(Codex.read_line(&mut state, &lines[0]));
        assert_eq!(
            (t.text.as_str(), t.session.as_str(), t.branch.as_str()),
            ("and the docs", ID, "feat")
        );
    }
}
