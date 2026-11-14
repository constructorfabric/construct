use std::env;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::{self, Deserializer, SeqAccess, Visitor};

use crate::text::{entries_of, is_jsonl_file, join_texts, root_under, starts_with_marker};
use crate::{FileState, Record, Role, Tool};

/// What the harness puts into user lines that the user did not type.
const HARNESS_TAGS: [&str; 8] = [
    "<command-name>",
    "<command-message>",
    "<local-command-stdout>",
    "<local-command-caveat>",
    "<system-reminder>",
    "<task-notification>",
    "<cross-session-message",
    "[Request interrupted",
];

/// Claude Code: `<config>/projects/<project>/<session>.jsonl`, where `<config>` is
/// `$CLAUDE_CONFIG_DIR` or `~/.claude`.
pub struct ClaudeCode;

impl ClaudeCode {
    fn root_in(home: &Path, config_dir: Option<OsString>) -> PathBuf {
        root_under(home, config_dir, ".claude", "projects")
    }
}

/// The text of a prompt typed while a turn was running, which Claude Code logs as an
/// attachment and not as a user line. Prompts of other origins are not the user's.
fn queued_prompt(attachment: Attachment) -> Option<String> {
    let human = attachment.origin.is_some_and(|o| o.kind == "human");
    if attachment.kind != "queued_command" || attachment.command_mode != "prompt" || !human {
        return None;
    }
    attachment
        .prompt
        .filter(|p| !p.is_empty() && !has_harness_tag(p))
}

fn has_harness_tag(text: &str) -> bool {
    starts_with_marker(text, &HARNESS_TAGS)
}

/// The `text` blocks of a message, one after the other; in a user message the harness's
/// own blocks are left out. `None` when no text is left.
fn join_text(role: Role, blocks: &[Block]) -> Option<String> {
    join_texts(
        blocks
            .iter()
            .filter(|b| b.kind == "text")
            .filter_map(|b| b.text.as_deref())
            .filter(|t| role == Role::Assistant || !has_harness_tag(t)),
    )
}

impl Tool for ClaudeCode {
    fn name(&self) -> &'static str {
        "claude-code"
    }

    fn root(&self, home: &Path) -> PathBuf {
        Self::root_in(home, env::var_os("CLAUDE_CONFIG_DIR"))
    }

    fn find_files(&self, root: &Path) -> io::Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        for project in entries_of(root)? {
            if project.is_dir() {
                files.extend(
                    entries_of(&project)?
                        .into_iter()
                        .filter(|f| is_jsonl_file(f)),
                );
            }
        }
        files.sort();
        Ok(files)
    }

    fn read_line(&self, state: &mut FileState, line: &str) -> Option<Record> {
        let line = serde_json::from_str::<Line>(line).ok()?;
        if !matches!(line.kind.as_str(), "user" | "assistant" | "attachment") {
            return None;
        }
        if let Some(id) = line.session_id.filter(|s| !s.is_empty()) {
            state.session = id;
        }
        if let Some(cwd) = line.cwd.filter(|s| !s.is_empty()) {
            state.folder = cwd;
        }
        if let Some(branch) = line.git_branch {
            state.branch = branch;
        }
        let time = line.timestamp?;
        let name = self.name();
        if line.kind == "attachment" {
            let text = queued_prompt(line.attachment?)?;
            return Some(state.turn(name, time, Role::User, text));
        }
        if line.is_sidechain {
            return Some(state.mark(name, time));
        }
        let typed = !line.is_meta && !line.is_compact_summary;
        let content = line.message.and_then(|m| m.content);
        let (role, text) = match (line.kind.as_str(), content) {
            ("user", Some(Content::Text(t))) if typed && !t.is_empty() && !has_harness_tag(&t) => {
                (Role::User, t)
            }
            ("user", Some(Content::Blocks(blocks))) if typed => {
                match join_text(Role::User, &blocks) {
                    Some(t) => (Role::User, t),
                    None => return Some(state.mark(name, time)),
                }
            }
            ("assistant", Some(Content::Blocks(blocks))) => {
                match join_text(Role::Assistant, &blocks) {
                    Some(t) => (Role::Assistant, t),
                    None => return Some(state.mark(name, time)),
                }
            }
            ("assistant", Some(Content::Text(t))) if !t.is_empty() => (Role::Assistant, t),
            _ => return Some(state.mark(name, time)),
        };
        Some(state.turn(name, time, role, text))
    }
}

/// The few fields of a line that are read. Fields that are not listed here, tool results
/// among them, are skipped while the line is parsed and not kept.
#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type", default)]
    kind: String,
    timestamp: Option<String>,
    cwd: Option<String>,
    #[serde(rename = "gitBranch")]
    git_branch: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    #[serde(rename = "isSidechain", default)]
    is_sidechain: bool,
    #[serde(rename = "isMeta", default)]
    is_meta: bool,
    #[serde(rename = "isCompactSummary", default)]
    is_compact_summary: bool,
    message: Option<Message>,
    attachment: Option<Attachment>,
}

#[derive(Deserialize)]
struct Message {
    content: Option<Content>,
}

#[derive(Deserialize)]
struct Attachment {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(rename = "commandMode", default)]
    command_mode: String,
    origin: Option<Origin>,
    prompt: Option<String>,
}

#[derive(Deserialize)]
struct Origin {
    #[serde(default)]
    kind: String,
}

#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type", default)]
    kind: String,
    text: Option<String>,
}

enum Content {
    Text(String),
    Blocks(Vec<Block>),
}

impl<'de> Deserialize<'de> for Content {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Content;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string or a list of blocks")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Content, E> {
                Ok(Content::Text(v.to_owned()))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Content, A::Error> {
                let mut blocks = Vec::new();
                while let Some(block) = seq.next_element::<Block>()? {
                    blocks.push(block);
                }
                Ok(Content::Blocks(blocks))
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_new_lines;
    use crate::text::Scratch;
    use crate::{Mark, Turn};

    const T: &str = "2026-11-11T10:15:30.123Z";

    fn line(kind: &str, extra: &str, content: &str) -> String {
        format!(
            r#"{{"type":"{kind}","timestamp":"{T}","cwd":"/w/app","gitBranch":"feat","sessionId":"s-1"{extra},"message":{{"role":"x","content":{content}}}}}"#
        )
    }

    fn queued(extra: &str) -> String {
        format!(
            r#"{{"type":"attachment","timestamp":"{T}","sessionId":"s-1","attachment":{{"type":"queued_command","commandMode":"prompt","origin":{{"kind":"human"}},"prompt":"also add a test"{extra}}}}}"#
        )
    }

    fn parse(l: &str) -> Option<Record> {
        ClaudeCode.read_line(&mut FileState::default(), l)
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

    fn assert_mark(r: Option<Record>) {
        let m = mark(r);
        assert_eq!((m.session.as_str(), m.folder.as_str()), ("s-1", "/w/app"));
        assert_eq!((m.branch.as_str(), m.time.as_str()), ("feat", T));
    }

    #[test]
    fn prompt_as_string() {
        let t = turn(parse(&line("user", "", r#""fix the build\nplease""#)));
        assert_eq!(t.role, Role::User);
        assert_eq!(t.text, "fix the build\nplease");
        assert_eq!(
            (t.tool.as_str(), t.session.as_str()),
            ("claude-code", "s-1")
        );
        assert_eq!(
            (t.folder.as_str(), t.branch.as_str(), t.time.as_str()),
            ("/w/app", "feat", T)
        );
    }

    #[test]
    fn prompt_as_text_list() {
        let c = r#"[{"type":"text","text":"first"},{"type":"image","source":{"data":"AAAA"}},{"type":"text","text":"second"}]"#;
        let t = turn(parse(&line("user", "", c)));
        assert_eq!((t.role, t.text.as_str()), (Role::User, "first\n\nsecond"));
    }

    #[test]
    fn tool_result_is_a_mark_only() {
        let c = r#"[{"type":"tool_result","tool_use_id":"t1","content":"output that is long"}]"#;
        assert_mark(parse(&line("user", "", c)));
    }

    #[test]
    fn assistant_text_is_a_turn() {
        let c = r#"[{"type":"text","text":"Done. The build passes."}]"#;
        let t = turn(parse(&line("assistant", "", c)));
        assert_eq!(
            (t.role, t.text.as_str()),
            (Role::Assistant, "Done. The build passes.")
        );
    }

    #[test]
    fn assistant_text_as_a_string_is_a_turn() {
        let t = turn(parse(&line("assistant", "", r#""Done.""#)));
        assert_eq!((t.role, t.text.as_str()), (Role::Assistant, "Done."));
    }

    #[test]
    fn assistant_text_that_looks_like_an_envelope_is_still_a_turn() {
        let c = r#"[{"type":"text","text":"<system-reminder> is what the harness adds"}]"#;
        assert_eq!(turn(parse(&line("assistant", "", c))).role, Role::Assistant);
    }

    #[test]
    fn empty_or_missing_content_is_a_mark_only() {
        assert_mark(parse(&line("assistant", "", r#""""#)));
        assert_mark(parse(&line("assistant", "", "[]")));
        assert_mark(parse(&line("user", "", r#""""#)));
        assert_mark(parse(&line("user", "", "null")));
        let no_message = format!(
            r#"{{"type":"assistant","timestamp":"{T}","cwd":"/w/app","gitBranch":"feat","sessionId":"s-1"}}"#
        );
        assert_mark(parse(&no_message));
    }

    #[test]
    fn thinking_and_tool_use_are_marks_only() {
        let thinking = r#"[{"type":"thinking","thinking":"hmm","signature":"x"}]"#;
        let tool_use = r#"[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]"#;
        assert_mark(parse(&line("assistant", "", thinking)));
        assert_mark(parse(&line("assistant", "", tool_use)));
    }

    #[test]
    fn sidechain_is_a_mark_only() {
        let sidechain = r#","isSidechain":true"#;
        assert_mark(parse(&line("user", sidechain, r#""subagent prompt""#)));
        let c = r#"[{"type":"text","text":"subagent answer"}]"#;
        assert_mark(parse(&line("assistant", sidechain, c)));
    }

    #[test]
    fn harness_blocks_around_a_prompt_are_left_out() {
        let c = r#"[{"type":"text","text":"<system-reminder>note</system-reminder>"},{"type":"text","text":"the prompt"},{"type":"text","text":"<command-message>x</command-message>"}]"#;
        assert_eq!(turn(parse(&line("user", "", c))).text, "the prompt");
    }

    #[test]
    fn meta_and_compaction_summary_are_marks_only() {
        let meta = r#","isMeta":true"#;
        assert_mark(parse(&line("user", meta, r#""Caveat: injected""#)));
        let c = r#"[{"type":"text","text":"Base directory for this skill: /x"}]"#;
        assert_mark(parse(&line("user", meta, c)));
        let summary = r#""This session is being continued from a previous conversation...""#;
        assert_mark(parse(&line("user", r#","isCompactSummary":true"#, summary)));
    }

    #[test]
    fn harness_envelopes_are_marks_only() {
        for tag in HARNESS_TAGS {
            let as_string = format!("\"{tag}\\nrest of the envelope\"");
            assert_mark(parse(&line("user", "", &as_string)));
            let as_block = format!(r#"[{{"type":"text","text":"{tag} x"}}]"#);
            assert_mark(parse(&line("user", "", &as_block)));
        }
    }

    #[test]
    fn an_envelope_tag_is_found_after_leading_whitespace_only() {
        assert_mark(parse(&line("user", "", r#""  \n<command-name>/clear""#)));
        let t = turn(parse(&line(
            "user",
            "",
            r#""see <system-reminder> in the log""#,
        )));
        assert_eq!(t.text, "see <system-reminder> in the log");
    }

    #[test]
    fn a_prompt_typed_while_a_turn_runs_is_a_user_turn() {
        let t = turn(parse(&queued("")));
        assert_eq!((t.role, t.text.as_str()), (Role::User, "also add a test"));
        assert_eq!((t.session.as_str(), t.time.as_str()), ("s-1", T));
    }

    #[test]
    fn queued_commands_that_the_user_did_not_type_are_nothing() {
        let from_task = queued("").replace(r#""kind":"human""#, r#""kind":"task-notification""#);
        let slash = queued("").replace(r#""commandMode":"prompt""#, r#""commandMode":"bash""#);
        let envelope = queued("").replace("also add a test", "<task-notification>done");
        let no_origin = queued("").replace(r#""origin":{"kind":"human"},"#, "");
        let other = queued("").replace("queued_command", "hook_success");
        for l in [from_task, slash, envelope, no_origin, other] {
            assert!(parse(&l).is_none(), "{l}");
        }
    }

    #[test]
    fn other_types_are_ignored() {
        for kind in [
            "mode",
            "queue-operation",
            "attachment",
            "pr-link",
            "last-prompt",
        ] {
            let l = format!(r#"{{"type":"{kind}","timestamp":"{T}","sessionId":"s-1"}}"#);
            assert!(parse(&l).is_none(), "{kind}");
        }
    }

    #[test]
    fn a_line_without_a_time_is_ignored() {
        let l = r#"{"type":"user","sessionId":"s-1","message":{"content":"hi"}}"#;
        assert!(parse(l).is_none());
    }

    #[test]
    fn unparseable_line_is_ignored() {
        assert!(parse(r#"{"type":"user","timestamp":"2026-11"#).is_none());
        assert!(parse("not json").is_none());
    }

    #[test]
    fn session_folder_and_branch_carry_over_to_later_lines() {
        let mut st = FileState::default();
        let first = line("user", "", r#""one""#);
        assert_eq!(turn(ClaudeCode.read_line(&mut st, &first)).session, "s-1");
        let bare = format!(
            r#"{{"type":"assistant","timestamp":"{T}","message":{{"content":[{{"type":"tool_use"}}]}}}}"#
        );
        let m = mark(ClaudeCode.read_line(&mut st, &bare));
        assert_eq!(
            (m.session.as_str(), m.folder.as_str(), m.branch.as_str()),
            ("s-1", "/w/app", "feat")
        );
        let moved = r#"{"type":"user","timestamp":"2026-11-11T10:00:00Z","cwd":"/other","sessionId":"s-2","message":{"content":"hi"}}"#;
        let t = turn(ClaudeCode.read_line(&mut st, moved));
        assert_eq!(
            (t.session.as_str(), t.folder.as_str(), t.branch.as_str()),
            ("s-2", "/other", "feat")
        );
    }

    #[test]
    fn finds_session_files_in_order_and_honours_the_config_dir() {
        let dir = Scratch::new("claude-find");
        dir.write("projects/-w-app/b.jsonl", "");
        dir.write("projects/-w-app/a.jsonl", "");
        dir.write("projects/-w-app/notes.txt", "");
        dir.write("projects/-w-lib/c.jsonl", "");
        dir.write("projects/stray.jsonl", "");
        let projects = dir.path().join("projects");
        let files = ClaudeCode.find_files(&projects).unwrap();
        assert_eq!(
            files,
            [
                projects.join("-w-app/a.jsonl"),
                projects.join("-w-app/b.jsonl"),
                projects.join("-w-lib/c.jsonl")
            ]
        );
        assert!(
            ClaudeCode
                .find_files(&dir.path().join("none"))
                .unwrap()
                .is_empty()
        );
        let home = Path::new("/h");
        let default = Path::new("/h/.claude/projects");
        assert_eq!(ClaudeCode::root_in(home, None), default);
        assert_eq!(ClaudeCode::root_in(home, Some("".into())), default);
        assert_eq!(
            ClaudeCode::root_in(home, Some("/cfg".into())),
            Path::new("/cfg/projects")
        );
    }

    #[test]
    fn a_session_file_is_read_in_two_steps_from_the_saved_offset() {
        let dir = Scratch::new("claude-read");
        let first = [
            line("user", "", r#""fix the build""#),
            "{damaged".to_owned(),
            line("assistant", "", r#"[{"type":"text","text":"Done."}]"#),
            queued(""),
        ];
        let path = dir.write("projects/-w-app/s-1.jsonl", &(first.join("\n") + "\n"));
        let files = ClaudeCode.find_files(&dir.path().join("projects")).unwrap();
        assert_eq!(files, std::slice::from_ref(&path));

        let mut state = FileState::default();
        let (lines, offset) = read_new_lines(&path, 0).unwrap();
        let records: Vec<Record> = lines
            .iter()
            .filter_map(|l| ClaudeCode.read_line(&mut state, l))
            .collect();
        let texts: Vec<&str> = records
            .iter()
            .filter_map(|r| match r {
                Record::Turn(t) => Some(t.text.as_str()),
                Record::Mark(_) => None,
            })
            .collect();
        assert_eq!(texts, ["fix the build", "Done.", "also add a test"]);

        let more = line("user", "", r#""and the docs""#);
        std::fs::write(&path, first.join("\n") + "\n" + &more + "\n").unwrap();
        let (lines, _) = read_new_lines(&path, offset).unwrap();
        assert_eq!(lines.len(), 1);
        let t = turn(ClaudeCode.read_line(&mut state, &lines[0]));
        assert_eq!(
            (t.text.as_str(), t.session.as_str()),
            ("and the docs", "s-1")
        );
    }
}
