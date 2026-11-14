use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::{self, Deserializer, SeqAccess, Visitor};

use crate::text::{join_texts, root_under};
use crate::{FileState, Listing, Mark, Parsed, Record, Role, Tool, Turn};

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

fn has_harness_tag(text: &str) -> bool {
    let text = text.trim_start();
    HARNESS_TAGS.iter().any(|tag| text.starts_with(tag))
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

    fn find_files(&self, root: &Path) -> io::Result<Listing> {
        let mut listing = Listing::default();
        let projects = match fs::read_dir(root) {
            Ok(dir) => dir,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(listing),
            Err(e) => return Err(e),
        };
        for project in projects {
            let project = match project {
                Ok(p) => p.path(),
                Err(e) => {
                    listing
                        .problems
                        .push(format!("cannot list {}: {e}", root.display()));
                    continue;
                }
            };
            if !project.is_dir() {
                continue;
            }
            let entries = match fs::read_dir(&project) {
                Ok(entries) => entries,
                // A project directory that went away since the listing is not a problem.
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => {
                    listing
                        .problems
                        .push(format!("cannot list {}: {e}", project.display()));
                    continue;
                }
            };
            for file in entries {
                match file {
                    Ok(file) => {
                        let file = file.path();
                        if file.extension().is_some_and(|e| e == "jsonl") && file.is_file() {
                            listing.files.push(file);
                        }
                    }
                    Err(e) => listing
                        .problems
                        .push(format!("cannot list {}: {e}", project.display())),
                }
            }
        }
        listing.files.sort();
        Ok(listing)
    }

    fn open_file(&self, path: &Path) -> FileState {
        FileState {
            session: path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            ..FileState::default()
        }
    }

    fn parse_line(&self, state: &mut FileState, line: &str) -> Parsed {
        let Ok(line) = serde_json::from_str::<Line>(line) else {
            return Parsed::Skipped;
        };
        if line.kind != "user" && line.kind != "assistant" {
            return Parsed::Ignored;
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
        let Some(time) = line.timestamp else {
            return Parsed::Ignored;
        };
        let mark = || {
            Parsed::Record(Record::Mark(Mark {
                tool: self.name().into(),
                session: state.session.clone(),
                time: time.clone(),
                folder: state.folder.clone(),
                branch: state.branch.clone(),
            }))
        };
        if line.is_sidechain {
            return mark();
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
                    None => return mark(),
                }
            }
            ("assistant", Some(Content::Blocks(blocks))) => {
                match join_text(Role::Assistant, &blocks) {
                    Some(t) => (Role::Assistant, t),
                    None => return mark(),
                }
            }
            ("assistant", Some(Content::Text(t))) if !t.is_empty() => (Role::Assistant, t),
            _ => return mark(),
        };
        Parsed::Record(Record::Turn(Turn {
            tool: self.name().into(),
            session: state.session.clone(),
            time,
            folder: state.folder.clone(),
            branch: state.branch.clone(),
            role,
            text,
        }))
    }
}

/// The few fields of a line that are read; everything else, tool results included, is
/// skipped while parsing and never held in memory.
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
}

#[derive(Deserialize)]
struct Message {
    content: Option<Content>,
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

            fn visit_string<E: de::Error>(self, v: String) -> Result<Content, E> {
                Ok(Content::Text(v))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Content, A::Error> {
                let mut blocks = Vec::new();
                while let Some(block) = seq.next_element::<Block>()? {
                    blocks.push(block);
                }
                Ok(Content::Blocks(blocks))
            }

            fn visit_unit<E: de::Error>(self) -> Result<Content, E> {
                Ok(Content::Blocks(Vec::new()))
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "2026-11-11T10:15:30.123Z";

    fn line(kind: &str, extra: &str, content: &str) -> String {
        format!(
            r#"{{"type":"{kind}","timestamp":"{T}","cwd":"/w/app","gitBranch":"feat","sessionId":"s-1"{extra},"message":{{"role":"x","content":{content}}}}}"#
        )
    }

    fn state() -> FileState {
        ClaudeCode.open_file(Path::new("/h/.claude/projects/p/s-1.jsonl"))
    }

    fn parse(l: &str) -> Parsed {
        ClaudeCode.parse_line(&mut state(), l)
    }

    fn turn(p: Parsed) -> Turn {
        match p {
            Parsed::Record(Record::Turn(t)) => t,
            other => panic!("not a turn: {other:?}"),
        }
    }

    fn assert_mark(p: &Parsed) {
        let Parsed::Record(Record::Mark(m)) = p else {
            panic!("not a mark: {p:?}")
        };
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
        assert_mark(&parse(&line("user", "", c)));
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
    fn thinking_and_tool_use_are_marks_only() {
        let thinking = r#"[{"type":"thinking","thinking":"hmm","signature":"x"}]"#;
        let tool_use = r#"[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]"#;
        assert_mark(&parse(&line("assistant", "", thinking)));
        assert_mark(&parse(&line("assistant", "", tool_use)));
    }

    #[test]
    fn sidechain_is_a_mark_only() {
        assert_mark(&parse(&line(
            "user",
            r#","isSidechain":true"#,
            r#""subagent prompt""#,
        )));
        let c = r#"[{"type":"text","text":"subagent answer"}]"#;
        assert_mark(&parse(&line("assistant", r#","isSidechain":true"#, c)));
    }

    #[test]
    fn harness_blocks_around_a_prompt_are_left_out() {
        let c = r#"[{"type":"text","text":"<system-reminder>note</system-reminder>"},{"type":"text","text":"the prompt"},{"type":"text","text":"<command-message>x</command-message>"}]"#;
        assert_eq!(turn(parse(&line("user", "", c))).text, "the prompt");
    }

    #[test]
    fn meta_and_compaction_summary_are_marks_only() {
        assert_mark(&parse(&line(
            "user",
            r#","isMeta":true"#,
            r#""Caveat: injected""#,
        )));
        let c = r#"[{"type":"text","text":"Base directory for this skill: /x"}]"#;
        assert_mark(&parse(&line("user", r#","isMeta":true"#, c)));
        let summary = r#""This session is being continued from a previous conversation...""#;
        assert_mark(&parse(&line(
            "user",
            r#","isCompactSummary":true"#,
            summary,
        )));
    }

    #[test]
    fn harness_envelopes_are_marks_only() {
        for tag in HARNESS_TAGS {
            let as_string = format!("\"{tag}\\nrest of the envelope\"");
            assert_mark(&parse(&line("user", "", &as_string)));
            let as_block = format!(r#"[{{"type":"text","text":"{tag} x"}}]"#);
            assert_mark(&parse(&line("user", "", &as_block)));
        }
        let c = r#"[{"type":"text","text":"<command-name>/clear</command-name>"}]"#;
        assert_mark(&parse(&line("user", "", c)));
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
            assert_eq!(parse(&l), Parsed::Ignored, "{kind}");
        }
    }

    #[test]
    fn unparseable_line_is_skipped() {
        assert_eq!(
            parse(r#"{"type":"user","timestamp":"2026-11"#),
            Parsed::Skipped
        );
        assert_eq!(parse("not json"), Parsed::Skipped);
        assert_eq!(
            ClaudeCode.parse_line(&mut state(), "not json"),
            Parsed::Skipped
        );
    }

    #[test]
    fn session_comes_from_the_file_name_then_the_line() {
        let mut st = state();
        assert_eq!(st.session, "s-1");
        let l = r#"{"type":"user","timestamp":"2026-11-11T10:00:00Z","cwd":"/other","sessionId":"s-2","message":{"content":"hi"}}"#;
        let t = turn(ClaudeCode.parse_line(&mut st, l));
        assert_eq!(
            (t.session.as_str(), t.folder.as_str(), t.branch.as_str()),
            ("s-2", "/other", "")
        );
    }

    #[test]
    fn finds_session_files_and_honours_the_config_dir() {
        let dir = std::env::temp_dir().join(format!("claude-code-find-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("projects/-w-app")).unwrap();
        fs::write(dir.join("projects/-w-app/a.jsonl"), "").unwrap();
        fs::write(dir.join("projects/-w-app/notes.txt"), "").unwrap();
        fs::write(dir.join("projects/stray.jsonl"), "").unwrap();
        let files = ClaudeCode.find_files(&dir.join("projects")).unwrap().files;
        assert_eq!(files, [dir.join("projects/-w-app/a.jsonl")]);
        assert!(
            ClaudeCode
                .find_files(&dir.join("missing"))
                .unwrap()
                .files
                .is_empty()
        );
        let home = Path::new("/h");
        assert_eq!(
            ClaudeCode::root_in(home, None),
            Path::new("/h/.claude/projects")
        );
        assert_eq!(
            ClaudeCode::root_in(home, Some("".into())),
            Path::new("/h/.claude/projects")
        );
        assert_eq!(
            ClaudeCode::root_in(home, Some("/cfg".into())),
            Path::new("/cfg/projects")
        );
    }

    /// Makes `dir` unreadable; false when that does nothing (root reads everything).
    #[cfg(unix)]
    fn lock(dir: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o000)).unwrap();
        fs::read_dir(dir).is_err()
    }

    #[cfg(unix)]
    #[test]
    fn a_project_that_cannot_be_listed_is_named_and_the_others_are_still_listed() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("claude-listing-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for project in ["bad", "good"] {
            fs::create_dir_all(root.join(project)).unwrap();
        }
        fs::write(root.join("good/a.jsonl"), "").unwrap();
        let locked = lock(&root.join("bad"));
        let listing = ClaudeCode.find_files(&root).unwrap();
        fs::set_permissions(root.join("bad"), fs::Permissions::from_mode(0o755)).unwrap();
        let _ = fs::remove_dir_all(&root);
        if !locked {
            return;
        }
        assert_eq!(listing.files, [root.join("good/a.jsonl")]);
        assert_eq!(listing.problems.len(), 1);
        assert!(
            listing.problems[0].starts_with(&format!("cannot list {}", root.join("bad").display())),
            "{:?}",
            listing.problems
        );
    }
}
