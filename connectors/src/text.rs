use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The sessions directory of a tool: `<override>/<sub>` when the environment variable
/// holds a non-empty value, else `<home>/<default_dir>/<sub>`.
pub fn root_under(
    home: &Path,
    env_value: Option<OsString>,
    default_dir: &str,
    sub: &str,
) -> PathBuf {
    match env_value.filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir).join(sub),
        None => home.join(default_dir).join(sub),
    }
}

/// The non-empty texts one after the other; `None` when none is left.
pub fn join_texts<'a>(texts: impl Iterator<Item = &'a str>) -> Option<String> {
    let texts: Vec<&str> = texts.filter(|t| !t.is_empty()).collect();
    (!texts.is_empty()).then(|| texts.join("\n\n"))
}

/// The longest tool name that is kept; a longer one is not a name.
const MAX_TOOL_NAME: usize = 128;

/// The tools that run a shell command, in any reader. For these the first word of the command
/// is kept next to the name (`bash:gh`), because it shows which trackers and code hosts a
/// person works with.
const SHELL_TOOLS: [&str; 5] = [
    "Bash",
    "shell",
    "shell_command",
    "exec_command",
    "container.exec",
];

/// A string field that is read when it is a plain string and as no value otherwise.
pub fn plain_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(match serde::Deserialize::deserialize(d)? {
        serde_json::Value::String(s) => Some(s),
        _ => None,
    })
}

/// Whether a word holds anything a shell reads as quoting or substitution. The command is
/// split on spaces without understanding such syntax, so a word that holds any of it may be
/// only a piece of an argument.
fn is_quoted(word: &str) -> bool {
    word.contains(['"', '\'', '\\', '$', '`', '(', ')'])
}

/// The program a command line starts with: the first word without its folder, after any
/// `NAME=value` settings. `None` when it is not a plain program name, or when a setting or the
/// word itself uses quoting or substitution, so a piece of an argument is never kept.
fn first_word(command: &str) -> Option<&str> {
    let mut words = command.split_whitespace();
    let word = loop {
        let word = words.next()?;
        match word.split_once('=') {
            Some((name, value)) if is_setting(name) => {
                if is_quoted(value) {
                    return None;
                }
            }
            _ => break word,
        }
    };
    if is_quoted(word) {
        return None;
    }
    let program = word.rsplit('/').next().unwrap_or(word);
    let plain = !program.is_empty()
        && program.len() <= 32
        && program
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    plain.then_some(program)
}

fn is_setting(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Adds the name of a tool the model asked to run to `tools`, once. For a shell tool,
/// `command` is its command line and the program it starts is added as `bash:<program>`
/// (`bash:gh`) in the place of the plain name. Only these names are kept, never the rest of
/// the command or any other argument or output.
pub fn add_tool_name(tools: &mut Vec<String>, name: &str, command: Option<&str>) {
    let name = name.trim();
    if name.is_empty() || name.len() > MAX_TOOL_NAME || name.chars().any(char::is_control) {
        return;
    }
    let program = SHELL_TOOLS
        .contains(&name)
        .then(|| command.and_then(first_word))
        .flatten();
    let label = match program {
        Some(program) => format!("bash:{program}"),
        None => name.to_owned(),
    };
    if !tools.contains(&label) {
        tools.push(label);
    }
}

/// The program of a command given as a list of words, as a shell tool may send it: a shell
/// started with `-c` or `-lc` stands for the command line that follows.
pub fn argv_command(argv: &[String]) -> Option<&str> {
    match argv {
        [shell, flag, line, ..]
            if matches!(first_word(shell), Some("bash" | "sh" | "zsh"))
                && matches!(flag.as_str(), "-c" | "-lc" | "-ic") =>
        {
            Some(line)
        }
        [program, ..] => Some(program),
        [] => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(calls: &[(&str, Option<&str>)]) -> Vec<String> {
        let mut tools = Vec::new();
        for (name, command) in calls {
            add_tool_name(&mut tools, name, *command);
        }
        tools
    }

    #[test]
    fn a_shell_tool_is_named_by_the_program_of_its_command_and_nothing_else() {
        assert_eq!(
            names(&[
                ("Bash", Some("gh pr list --token abc")),
                ("Bash", Some("GH_HOST=x /usr/bin/glab mr list")),
                ("Bash", Some("gh issue view 1")),
                ("Read", None),
                ("mcp__tracker__get_issue", None),
            ]),
            ["bash:gh", "bash:glab", "Read", "mcp__tracker__get_issue"]
        );
    }

    #[test]
    fn a_command_without_a_plain_program_leaves_the_tool_name() {
        assert_eq!(names(&[("Bash", Some("$(evil) x"))]), ["Bash"]);
        assert_eq!(names(&[("Bash", Some("  "))]), ["Bash"]);
        assert_eq!(names(&[("Bash", None)]), ["Bash"]);
    }

    #[test]
    fn quoting_and_substitution_never_leave_a_piece_of_an_argument_as_the_program() {
        for command in [
            r#"PASSWORD="correct horse battery staple" gh pr list"#,
            "V=$(git rev-parse HEAD) make",
            "V=`git rev-parse HEAD` make",
            r"V=a\ b gh",
            "'/Users/ann/Secret Project/run.sh' go",
            r#""gh" pr"#,
            "$(gh) x",
            r"my\ tool run",
        ] {
            assert_eq!(names(&[("Bash", Some(command))]), ["Bash"], "{command}");
        }
        assert_eq!(names(&[("Bash", Some("A=1 B=two gh pr"))]), ["bash:gh"]);
    }

    #[test]
    fn names_that_are_not_names_are_left_out() {
        assert!(names(&[("", None), ("a\nb", None), (&"x".repeat(200), None)]).is_empty());
    }

    #[test]
    fn a_command_list_through_a_shell_reads_as_the_line_it_runs() {
        let argv = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            argv_command(&argv(&["bash", "-lc", "gh pr view"])),
            Some("gh pr view")
        );
        assert_eq!(argv_command(&argv(&["git", "status"])), Some("git"));
        assert_eq!(argv_command(&[]), None);
    }
}
