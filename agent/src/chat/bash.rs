//! The `bash` tool: one read-only command, run without a shell.
//!
//! The command is split into words by this module, never by a shell, so a pipe, a redirect, a
//! variable or a substitution is an error and cannot happen. The first word must be on a short list,
//! and every option must be one that list knows for that command: what is not listed is refused,
//! which is how `find -delete`, `grep -f` or `git commit` are kept out. A path in the command must
//! lead into a folder of the user and must not be a secret. The command runs in a folder of the
//! user, with a fixed minimal environment (a few variables for `PATH`, the locale and `git`, and
//! nothing else), no input, at most [`TIMEOUT`] and at most [`MAX_STREAM`] bytes read from each of
//! its outputs.
//!
//! `git` gets more care, because it can print the content of files. It runs only inside a
//! repository that is under the folder the user listed (git is told not to look higher), shows
//! revisions only as commits, so a blob or a `rev:path` is refused, and is always given exclusions
//! for the names that hold keys and credentials, in any case. The paths given to `git` are checked
//! as text only: git does not follow a link inside the repository when it shows a file, and it
//! cannot leave the repository, so a text check is enough there.

use std::fmt::Write as _;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, LazyLock};
use std::thread;
use std::time::{Duration, Instant};

use regex::Regex;
use serde::Deserialize;

use super::files::parse;
use super::machine::{Folders, finish, git_excludes, grep_skips, is_secret_name};
use super::tool::ChatTool;

const ALLOWED: &str = "git, ls, cat, grep, find, jq";
const NO_SHELL: &str = "there is no shell: pipes, redirects, `;`, `&`, variables, backslashes and substitutions are not supported; give one plain command";
const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_STREAM: u64 = 1024 * 1024;
/// The longest a check of `git` before the command runs.
/// Most revisions one `git log`, `show` or `diff` may name; each is checked by a run of `git`.
const MAX_REVISIONS: usize = 10;
const PROBE_TIME: Duration = Duration::from_secs(5);
const OUTSIDE_FOLDER: &str = "a path in a git command must stay inside the folder";

/// The subcommands of `git` that can run.
const GIT_SUBCOMMANDS: [&str; 11] = [
    "remote",
    "branch",
    "tag",
    "log",
    "show",
    "diff",
    "status",
    "ls-files",
    "rev-parse",
    "shortlog",
    "describe",
];
/// Options a submodule `git` can run through, which `git` takes cut short to a single letter.
const GIT_DENIED_AT_ANY_LENGTH: [&str; 2] = ["--dirty", "--broken"];
/// Long options of `git` that write a file, run a program, read outside the folder or check a
/// signature. An abbreviation of four characters or more counts, because `git` takes one.
const GIT_DENIED: [&str; 18] = [
    "--output",
    "--ext-diff",
    "--textconv",
    "--no-index",
    "--exec",
    "--open-files-in-pager",
    "--exclude-from",
    "--exclude-per-directory",
    "--git-dir",
    "--work-tree",
    "--show-signature",
    "--contents",
    "--ignore-revs-file",
    // These two print a diff that the exclusions do not filter: of every commit that touches
    // the pathspec, and inside a submodule.
    "--full-diff",
    "--submodule",
    // Starts a child `git` in each submodule, which reads the submodule's own configuration.
    "--recurse-submodules",
    // `describe` starts one in each submodule to see whether it is dirty.
    "--dirty",
    "--broken",
];

const LS_SHORT: &str = "lahA1trSdFRis";
const CAT_SHORT: &str = "nbs";
const GREP_PLAIN: &str = "inrlLcwvHhEFIsoqx";
const GREP_VALUE: &str = "ABCem";
const GREP_LONG: [&str; 14] = [
    "--ignore-case",
    "--line-number",
    "--recursive",
    "--files-with-matches",
    "--files-without-match",
    "--count",
    "--word-regexp",
    "--invert-match",
    "--fixed-strings",
    "--extended-regexp",
    "--no-messages",
    "--only-matching",
    "--with-filename",
    "--no-filename",
];
const GREP_LONG_VALUE: [&str; 4] = ["--include=", "--exclude=", "--exclude-dir=", "--max-count="];
const FIND_PLAIN: [&str; 10] = [
    "-print", "-not", "!", "-o", "-or", "-a", "-and", "(", ")", "-empty",
];
const FIND_VALUE: [&str; 9] = [
    "-name",
    "-iname",
    "-path",
    "-ipath",
    "-type",
    "-maxdepth",
    "-mindepth",
    "-size",
    "-mtime",
];
const JQ_SHORT: &str = "rcSMnsej";
const JQ_LONG: [&str; 6] = [
    "--raw-output",
    "--compact-output",
    "--sort-keys",
    "--slurp",
    "--null-input",
    "--exit-status",
];

pub(super) struct Bash(pub(super) Arc<Folders>);

#[derive(Deserialize)]
struct Arguments {
    command: String,
    directory: String,
}

impl ChatTool for Bash {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn description(&self) -> &'static str {
        super::tool::description(self.name())
    }

    fn schema(&self) -> &'static str {
        super::tool::schema(self.name())
    }

    fn line(&self, arguments: &str) -> String {
        let command = serde_json::from_str::<serde_json::Value>(arguments)
            .ok()
            .and_then(|v| v["command"].as_str().map(str::to_owned));
        match command {
            Some(command) => {
                let words: Vec<&str> = command.split_whitespace().take(2).collect();
                format!("Ran {}", words.join(" "))
            }
            None => self.name().to_string(),
        }
    }

    fn run(&self, arguments: &str) -> Result<String, String> {
        let args: Arguments = parse(arguments)?;
        let directory = self.0.check(&args.directory, None)?;
        if !directory.is_dir() {
            return Err("the directory is not a folder".to_string());
        }
        let words = split(&args.command)?;
        // Git must not look for a repository above the folder the user listed.
        let ceiling = self
            .0
            .root_of(&directory)
            .and_then(|root| root.parent().map(Path::to_path_buf));
        let vet = Vet {
            folders: &self.0,
            directory: &directory,
            ceiling: ceiling.as_deref(),
        };
        let cmd = vet.plan(&words)?;
        execute(&cmd, &directory, ceiling.as_deref(), TIMEOUT)
    }
}

/// The words of `command`, split like a shell would for quotes only. Anything a shell would act on
/// outside quotes is an error.
fn split(command: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some('\'') if c == '\'' => quote = None,
            Some('"') if c == '"' => quote = None,
            Some('"') if c == '\\' => match chars.peek() {
                Some(&next) if next == '"' || next == '\\' => {
                    word.push(next);
                    chars.next();
                }
                _ => word.push(c),
            },
            Some(_) => word.push(c),
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    started = true;
                }
                '\n' | '\r' | '|' | '&' | ';' | '<' | '>' | '`' | '$' | '\\' => {
                    return Err(NO_SHELL.to_string());
                }
                c if c.is_whitespace() => {
                    if started {
                        words.push(std::mem::take(&mut word));
                        started = false;
                    }
                }
                c => {
                    word.push(c);
                    started = true;
                }
            },
        }
    }
    if quote.is_some() {
        return Err("a quote is not closed".to_string());
    }
    if started {
        words.push(word);
    }
    if words.is_empty() {
        return Err("the command is empty".to_string());
    }
    Ok(words)
}

/// Judges a split command.
struct Vet<'a> {
    folders: &'a Folders,
    directory: &'a Path,
    /// The folder git must not go up into, when looking for a repository.
    ceiling: Option<&'a Path>,
}

fn unsupported(what: &str) -> String {
    format!("the option `{what}` is not allowed")
}

impl Vet<'_> {
    /// The program and arguments to run, when the command is allowed.
    fn plan(&self, words: &[String]) -> Result<Vec<String>, String> {
        let Some((program, rest)) = words.split_first() else {
            return Err("the command is empty".to_string());
        };
        match program.as_str() {
            "git" => self.git(rest),
            "ls" => self.ls(rest),
            "cat" => self.cat(rest),
            "grep" => self.grep(rest),
            "find" => self.find(rest),
            "jq" => self.jq(rest),
            other => Err(format!(
                "`{}` is not allowed; the commands that can run are: {ALLOWED}",
                other.chars().take(40).collect::<String>()
            )),
        }
    }

    /// Where `arg` leads, as text for the command.
    fn path(&self, arg: &str) -> Result<String, String> {
        self.folders
            .check(arg, Some(self.directory))
            .map(|path| path.to_string_lossy().into_owned())
    }

    fn ls(&self, args: &[String]) -> Result<Vec<String>, String> {
        self.flags_and_paths("ls", LS_SHORT, args)
    }

    fn cat(&self, args: &[String]) -> Result<Vec<String>, String> {
        self.flags_and_paths("cat", CAT_SHORT, args)
    }

    /// `program` with the short flags in `allowed` and paths, nothing else.
    fn flags_and_paths(
        &self,
        program: &str,
        allowed: &str,
        args: &[String],
    ) -> Result<Vec<String>, String> {
        let mut cmd = vec![program.to_string()];
        for arg in args {
            if let Some(flags) = short_flags(arg) {
                check_flags(flags, allowed)?;
                cmd.push(arg.clone());
            } else if arg.starts_with('-') {
                return Err(unsupported(arg));
            } else {
                cmd.push(self.path(arg)?);
            }
        }
        Ok(cmd)
    }

    fn grep(&self, args: &[String]) -> Result<Vec<String>, String> {
        let mut cmd = vec!["grep".to_string()];
        let mut pattern_given = false;
        let mut paths = Vec::new();
        let mut rest = args.iter();
        while let Some(arg) = rest.next() {
            if GREP_LONG.contains(&arg.as_str())
                || GREP_LONG_VALUE.iter().any(|name| arg.starts_with(name))
            {
                cmd.push(arg.clone());
            } else if let Some(flags) = short_flags(arg) {
                for (at, flag) in flags.char_indices() {
                    if GREP_VALUE.contains(flag) {
                        cmd.push(format!("-{flag}"));
                        let attached = &flags[at + flag.len_utf8()..];
                        if flag == 'e' {
                            pattern_given = true;
                        }
                        if attached.is_empty() {
                            let value = rest
                                .next()
                                .ok_or_else(|| format!("the option `-{flag}` needs a value"))?;
                            cmd.push(value.clone());
                        } else {
                            cmd.push(attached.to_string());
                        }
                        break;
                    } else if GREP_PLAIN.contains(flag) {
                        cmd.push(format!("-{flag}"));
                    } else {
                        return Err(unsupported(&format!("-{flag}")));
                    }
                }
            } else if arg.starts_with('-') {
                return Err(unsupported(arg));
            } else if pattern_given {
                paths.push(self.path(arg)?);
            } else {
                pattern_given = true;
                cmd.push("-e".to_string());
                cmd.push(arg.clone());
            }
        }
        if !pattern_given {
            return Err("give a pattern to search for".to_string());
        }
        cmd.extend(grep_skips());
        cmd.push("--".to_string());
        cmd.extend(paths);
        Ok(cmd)
    }

    fn find(&self, args: &[String]) -> Result<Vec<String>, String> {
        let mut cmd = vec!["find".to_string()];
        let mut rest = args.iter().peekable();
        while let Some(arg) =
            rest.next_if(|a| !a.starts_with('-') && !["(", "!"].contains(&a.as_str()))
        {
            cmd.push(self.path(arg)?);
        }
        while let Some(arg) = rest.next() {
            if FIND_PLAIN.contains(&arg.as_str()) {
                cmd.push(arg.clone());
            } else if FIND_VALUE.contains(&arg.as_str()) {
                let value = rest
                    .next()
                    .ok_or_else(|| format!("the option `{arg}` needs a value"))?;
                cmd.push(arg.clone());
                cmd.push(value.clone());
            } else {
                return Err(unsupported(arg));
            }
        }
        Ok(cmd)
    }

    fn jq(&self, args: &[String]) -> Result<Vec<String>, String> {
        // `env` and `$ENV` print the environment; `import`, `include` and `modulemeta` read
        // other files by name, outside the paths checked here.
        static FORBIDDEN: LazyLock<Regex> = LazyLock::new(|| {
            #[allow(clippy::expect_used)] // A constant pattern.
            Regex::new(r"\b(?:env|ENV|import|include|modulemeta)\b").expect("a constant pattern")
        });
        let mut cmd = vec!["jq".to_string()];
        let mut filter = false;
        for arg in args {
            if JQ_LONG.contains(&arg.as_str()) {
                cmd.push(arg.clone());
            } else if let Some(flags) = short_flags(arg) {
                check_flags(flags, JQ_SHORT)?;
                cmd.push(arg.clone());
            } else if arg.starts_with('-') && arg != "-" {
                return Err(unsupported(arg));
            } else if !filter {
                if FORBIDDEN.is_match(arg) {
                    return Err(
                        "the filter reads the environment or other files, which is not allowed"
                            .to_string(),
                    );
                }
                filter = true;
                cmd.push(arg.clone());
            } else {
                cmd.push(self.path(arg)?);
            }
        }
        if !filter {
            return Err("give a jq filter".to_string());
        }
        Ok(cmd)
    }

    fn git(&self, args: &[String]) -> Result<Vec<String>, String> {
        let Some((sub, rest)) = args.split_first() else {
            return Err("give a git subcommand".to_string());
        };
        if !GIT_SUBCOMMANDS.contains(&sub.as_str()) {
            return Err(format!(
                "`git {}` is not allowed; the subcommands that can run are: {}",
                sub.chars().take(40).collect::<String>(),
                GIT_SUBCOMMANDS.join(", ")
            ));
        }
        match sub.as_str() {
            "remote" => git_remote(rest)?,
            "branch" => git_branch(rest)?,
            "tag" => git_tag(rest)?,
            "status" => git_status(rest)?,
            _ => {}
        }
        git_arguments(rest)?;
        // The output of a run of `git` that succeeded.
        let probe_out = |args: &[&str]| -> Option<Vec<u8>> {
            let cmd: Vec<String> = std::iter::once("git")
                .chain(args.iter().copied())
                .map(ToString::to_string)
                .collect();
            run_process(&cmd, self.directory, self.ceiling, PROBE_TIME)
                .ok()
                .filter(|done| done.status.is_some_and(|status| status.success()))
                .map(|done| done.out)
        };
        let probe = |args: &[&str]| probe_out(args).is_some();
        if !probe(&["rev-parse", "--git-dir"]) {
            return Err(
                "this folder is not inside a git repository that is in your folders".to_string(),
            );
        }
        // The configuration of the repository is not ours: it can name programs that `git` runs
        // by itself (filters, signature checks, hooks, a remote shell), and `-c` cannot turn a
        // filter off. Such a repository is not read.
        // A configuration that cannot be read in full is not read: the part left out may name a
        // program. A configuration over the cap closes the pipe, so git stops and the read fails.
        let config = probe_out(&["config", "--list", "--includes", "-z"])
            .ok_or_else(|| {
                "the configuration of this repository could not be read in full, so git is not run here".to_string()
            })?;
        check_config(&config)?;
        let mut cmd: Vec<String> = [
            "git",
            "--no-pager",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.pager=cat",
            "-c",
            "diff.submodule=short",
            sub,
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        if ["log", "show", "diff"].contains(&sub.as_str()) {
            // These print file content: show commits only, leave secret names out.
            let (before, after) = match rest.iter().position(|arg| arg == "--") {
                Some(at) => (&rest[..at], &rest[at + 1..]),
                None => (rest, &rest[rest.len()..]),
            };
            check_view(sub, before, &probe)?;
            cmd.extend(["--no-ext-diff".to_string(), "--no-textconv".to_string()]);
            if sub == "diff" {
                // `diff` starts a child `git` in a submodule that `submodule.<name>.ignore` (a
                // tracked `.gitmodules` file sets it) asks to look into, which `-c` does not beat.
                cmd.push("--ignore-submodules=all".to_string());
            }
            cmd.extend(before.iter().cloned());
            cmd.push("--".to_string());
            cmd.extend(after.iter().cloned());
            cmd.extend(git_excludes());
        } else {
            if sub == "status" {
                // `status` starts a child `git` in each submodule, which reads that submodule's own
                // configuration, where no check was made.
                cmd.push("--ignore-submodules=all".to_string());
            }
            cmd.extend(rest.iter().cloned());
        }
        Ok(cmd)
    }
}

/// The part of a `git log`, `show` or `diff` command before `--`: allowed options, and commits.
fn check_view(sub: &str, before: &[String], probe: &dyn Fn(&[&str]) -> bool) -> Result<(), String> {
    let mut value_next = false;
    let mut probes = 0;
    for arg in before {
        let was_value = std::mem::take(&mut value_next);
        if arg.starts_with('-') {
            if !view_option_allowed(sub, arg) {
                return Err(format!(
                    "the option `{}` is not allowed for `git {sub}`: only options that choose and format commits are",
                    arg.chars().take(40).collect::<String>()
                ));
            }
            value_next = ["-n", "--max-count", "--skip", "-U", "--unified"].contains(&arg.as_str());
        } else if was_value && arg.chars().all(|c| c.is_ascii_digit()) {
            // A number that is an option's value.
        } else {
            probes += 1;
            if probes > MAX_REVISIONS {
                return Err("too many revisions in one command".to_string());
            }
            if !is_commits(arg, probe) {
                return Err(format!(
                    "`{}` is not a commit: `git {sub}` takes commits before `--` and paths after it, and an option's value must be joined to it, as in `-n5`",
                    arg.chars().take(60).collect::<String>()
                ));
            }
        }
    }
    Ok(())
}

/// Whether `arg`, a revision or a range such as `a..b`, names only commits (a tag counts as the
/// commit it points to). `probe` runs `git` with the arguments and tells if it succeeded.
fn is_commits(arg: &str, probe: &dyn Fn(&[&str]) -> bool) -> bool {
    let sides: Vec<&str> = arg.split("...").flat_map(|part| part.split("..")).collect();
    sides.iter().all(|side| {
        let side = side.trim_start_matches('^');
        side.is_empty()
            || probe(&[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{side}^{{commit}}"),
            ])
    })
}

/// An error naming the first flag of the bundle `flags` that is not in `allowed`.
fn check_flags(flags: &str, allowed: &str) -> Result<(), String> {
    match flags.chars().find(|c| !allowed.contains(*c)) {
        Some(bad) => Err(unsupported(&format!("-{bad}"))),
        None => Ok(()),
    }
}

/// An error when the configuration `config` (`git config --list -z`) names programs for `git` to
/// run by itself.
fn check_config(config: &[u8]) -> Result<(), String> {
    if String::from_utf8_lossy(config)
        .split('\0')
        .filter_map(|entry| entry.split(['\n', '=']).next())
        .any(|key| runs_programs(&key.to_lowercase()))
    {
        return Err("the configuration of this repository names programs for git to run or a work tree elsewhere (`filter.*`, `gpg.*`, `core.hooksPath`, `core.sshCommand` or `core.worktree`), so git is not run here".to_string());
    }
    Ok(())
}

/// Whether a configuration key makes one of the allowed subcommands run a program by itself, or
/// read a work tree that is not the folder. (A gitdir or object store elsewhere is only more
/// history, which the exclusions filter like the rest.) The user's own configuration is never
/// read, so keys such as `diff.external` and `core.pager` are covered by command line options.
fn runs_programs(key: &str) -> bool {
    key.starts_with("filter.")
        || key.starts_with("gpg.")
        || key == "core.hookspath"
        || key == "core.sshcommand"
        || key == "core.worktree"
}

/// The options of `git log`, `show` and `diff` that choose and format commits and diffs. A list
/// of what is allowed, not of what is not: an option that adds objects or references as
/// revisions (`--indexed-objects`, `--tags`, `--objects`) prints content that no exclusion
/// filters.
const VIEW_FLAGS: [&str; 52] = [
    "-p",
    "--patch",
    "-u",
    "-s",
    "--no-patch",
    "--stat",
    "--shortstat",
    "--numstat",
    "--name-only",
    "--name-status",
    "--summary",
    "--compact-summary",
    "--oneline",
    "--abbrev-commit",
    "--no-abbrev-commit",
    "--graph",
    "--decorate",
    "--no-decorate",
    "--reverse",
    "--no-merges",
    "--merges",
    "--first-parent",
    "--follow",
    "--raw",
    "--cached",
    "--staged",
    "-w",
    "--ignore-all-space",
    "-b",
    "--ignore-space-change",
    "--ignore-blank-lines",
    "--no-color",
    "--minimal",
    "--patience",
    "--histogram",
    "--no-renames",
    "--find-renames",
    "-M",
    "--text",
    "-a",
    "--topo-order",
    "--date-order",
    "--author-date-order",
    "-i",
    "--regexp-ignore-case",
    "--all-match",
    "--invert-grep",
    "--relative-date",
    "--no-ext-diff",
    "--no-textconv",
    "-n",
    "--max-count",
];
/// Options of the list above that take a value.
const VIEW_VALUED: [&str; 17] = [
    "--format=",
    "--pretty=",
    "--date=",
    "--stat=",
    "--abbrev=",
    "--diff-filter=",
    "--max-count=",
    "--skip=",
    "--since=",
    "--until=",
    "--after=",
    "--before=",
    "--author=",
    "--committer=",
    "--grep=",
    "--unified=",
    "--find-renames=",
];
/// Options that join their value: `-n5`, `-U3`, `-Sword`, `-Gpattern`, `-M50`.
const VIEW_GLUED: [&str; 5] = ["-n", "-U", "-S", "-G", "-M"];
/// Options only `git log` takes, which walk more references; for `show` they are objects.
const LOG_REFS: [&str; 4] = ["--all", "--branches", "--tags", "--remotes"];

fn view_option_allowed(sub: &str, arg: &str) -> bool {
    let digits = |text: &str| !text.is_empty() && text.chars().all(|c| c.is_ascii_digit());
    VIEW_FLAGS.contains(&arg)
        || ["--skip", "-U", "--unified"].contains(&arg)
        || VIEW_VALUED.iter().any(|name| arg.starts_with(name))
        || VIEW_GLUED
            .iter()
            .any(|name| arg.len() > name.len() && arg.starts_with(name))
        || arg.strip_prefix('-').is_some_and(digits)
        || (sub == "log" && LOG_REFS.contains(&arg))
}

/// The flags of a bundle like `-la`, without the dash. `None` for a word that is not one.
fn short_flags(arg: &str) -> Option<&str> {
    let flags = arg.strip_prefix('-')?;
    (!flags.is_empty() && !flags.starts_with('-')).then_some(flags)
}

fn git_remote(args: &[String]) -> Result<(), String> {
    match args {
        [] => Ok(()),
        [flag] if flag == "-v" => Ok(()),
        [get, name] if get == "get-url" && !name.starts_with('-') => Ok(()),
        _ => Err(
            "`git remote` can run with no argument, with `-v`, or as `get-url <name>`".to_string(),
        ),
    }
}

fn git_branch(args: &[String]) -> Result<(), String> {
    const FLAGS: [&str; 9] = [
        "-a",
        "-r",
        "-v",
        "-vv",
        "--all",
        "--remotes",
        "--verbose",
        "--show-current",
        "--no-color",
    ];
    const LIST: [&str; 2] = ["-l", "--list"];
    const WITH_REVISION: [&str; 4] = ["--contains", "--merged", "--no-merged", "--points-at"];
    let listing = args.iter().any(|a| LIST.contains(&a.as_str()));
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if FLAGS.contains(&arg.as_str()) || LIST.contains(&arg.as_str()) {
            continue;
        }
        if WITH_REVISION.contains(&arg.as_str()) {
            match rest.next() {
                Some(value) if !value.starts_with('-') => continue,
                _ => return Err(format!("the option `{arg}` needs a revision")),
            }
        }
        if !arg.starts_with('-') && listing {
            continue;
        }
        return Err("`git branch` can only list: use `-a`, `-r`, `-v`, `--list [pattern]`, `--show-current`, `--contains <revision>`".to_string());
    }
    Ok(())
}

/// `git status` with the options that only name files. `-v` and `--verbose` print the diff of the
/// staged changes, which the exclusions do not filter, so the options are listed, not denied.
fn git_status(args: &[String]) -> Result<(), String> {
    const FLAGS: [&str; 12] = [
        "-s",
        "--short",
        "--porcelain",
        "--porcelain=v1",
        "--porcelain=v2",
        "-b",
        "--branch",
        "--ignored",
        "-z",
        "--no-renames",
        "-uno",
        "-unormal",
    ];
    for arg in args {
        let known = FLAGS.contains(&arg.as_str())
            || [
                "-uall",
                "--untracked-files=no",
                "--untracked-files=normal",
                "--untracked-files=all",
            ]
            .contains(&arg.as_str())
            || arg == "--";
        if !known && (arg.starts_with('-') || !args.contains(&"--".to_string())) {
            return Err("`git status` can run with `-s`, `--short`, `--porcelain`, `-b`, `--branch`, `-u<mode>`, `--ignored` and `-z`, then `--` and paths".to_string());
        }
    }
    Ok(())
}

fn git_tag(args: &[String]) -> Result<(), String> {
    let Some(first) = args.first() else {
        return Ok(());
    };
    if first != "-l" && first != "--list" {
        return Err("`git tag` can only list: start with `-l`".to_string());
    }
    let mut rest = args[1..].iter();
    while let Some(arg) = rest.next() {
        let digits_only = |text: &str| text.chars().all(|c| c.is_ascii_digit());
        let known = arg.starts_with("--sort=")
            || arg.strip_prefix("-n").is_some_and(digits_only)
            || !arg.starts_with('-');
        if arg == "--contains" {
            match rest.next() {
                Some(value) if !value.starts_with('-') => continue,
                _ => return Err("the option `--contains` needs a revision".to_string()),
            }
        }
        if !known {
            return Err(unsupported(arg));
        }
    }
    Ok(())
}

/// What no `git` subcommand may be given: options that write, run a program, read outside the
/// folder or check a signature, and words that lead outside the folder or to a secret.
fn git_arguments(args: &[String]) -> Result<(), String> {
    let mut after_dashes = false;
    for arg in args {
        if arg.contains("%G") {
            return Err("signature checks are not allowed".to_string());
        }
        if !after_dashes && arg == "--" {
            after_dashes = true;
            continue;
        }
        if !after_dashes && arg.starts_with('-') {
            let name = arg.split('=').next().unwrap_or(arg);
            // `-O<file>` reads an order file or opens files in a pager (a program), `-X<file>`
            // reads patterns from a file, `-L` follows a path by line range with its own path.
            // Git takes a long option cut short in some places (a negation such as `--no-ext` in
            // `log`, `show` and `diff`, more options in other subcommands); each command differs.
            // Four characters is a safe over-denial: it also stops a cut form such as `--ou` that
            // is no option at all.
            let denied = name.starts_with("-O")
                || name.starts_with("-L")
                || name.starts_with("-X")
                || (name.starts_with("--")
                    && name.len() >= 4
                    && GIT_DENIED.iter().any(|d| d.starts_with(name)))
                // Git takes a unique prefix of any length (`--b` is `--broken` in `describe`), so
                // for these two even `--b` counts.
                || (name.starts_with("--")
                    && name.len() >= 3
                    && GIT_DENIED_AT_ANY_LENGTH.iter().any(|d| d.starts_with(name)));
            if denied {
                return Err(unsupported(name));
            }
            continue;
        }
        if arg.starts_with('/') || arg.starts_with('~') || arg.split('/').any(|part| part == "..") {
            return Err(OUTSIDE_FOLDER.to_string());
        }
        // A revision names a file as `rev:path` or `:path`, so both `/` and `:` start a new part.
        if arg.split(['/', ':']).any(is_secret_name) {
            return Err("that path may hold keys or credentials and is not available".to_string());
        }
    }
    Ok(())
}

/// What a finished process left.
struct Done {
    out: Vec<u8>,
    err: Vec<u8>,
    /// `None` when it was stopped for running too long.
    status: Option<std::process::ExitStatus>,
}

/// Runs `cmd` in `directory` for at most `limit`, with the fixed environment. `ceiling` is the
/// folder git must not go up into.
fn run_process(
    cmd: &[String],
    directory: &Path,
    ceiling: Option<&Path>,
    limit: Duration,
) -> Result<Done, String> {
    let Some((program, rest)) = cmd.split_first() else {
        return Err("the command is empty".to_string());
    };
    let mut command = Command::new(program);
    command
        .args(rest)
        .current_dir(directory)
        .env_clear()
        .env("PATH", "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin")
        .env("LC_ALL", "C.UTF-8")
        // The user's own git configuration may name programs to run (a diff tool, a credential
        // helper), so git reads none of it.
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(ceiling) = ceiling {
        command.env("GIT_CEILING_DIRECTORIES", ceiling);
    }
    let mut child = command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("`{program}` is not installed on this machine")
        } else {
            "the command could not be started".to_string()
        }
    })?;
    let out = child
        .stdout
        .take()
        .map(|pipe| thread::spawn(move || read_capped(pipe)));
    let err = child
        .stderr
        .take()
        .map(|pipe| thread::spawn(move || read_capped(pipe)));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < limit => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    Ok(Done {
        out: out
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default(),
        err: err
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default(),
        status,
    })
}

/// Runs `cmd` in `directory` and returns what it printed, with the error output and the exit
/// status when there is a problem.
fn execute(
    cmd: &[String],
    directory: &Path,
    ceiling: Option<&Path>,
    limit: Duration,
) -> Result<String, String> {
    let done = run_process(cmd, directory, ceiling, limit)?;
    let Some(status) = done.status else {
        return Err(format!(
            "the command ran for more than {} seconds and was stopped",
            limit.as_secs_f32().round()
        ));
    };
    let mut text = String::from_utf8_lossy(&done.out).into_owned();
    if !done.err.is_empty() {
        text.push_str("\n[error output]\n");
        text.push_str(&String::from_utf8_lossy(&done.err));
    }
    if text.trim().is_empty() {
        text = "(no output)".to_string();
    }
    if !status.success() {
        let _ = write!(text, "\n[exit status {}]", status.code().unwrap_or(-1));
    }
    Ok(finish(&text))
}

/// At most [`MAX_STREAM`] bytes of `pipe`.
fn read_capped(pipe: impl Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    let _ = pipe.take(MAX_STREAM).read_to_end(&mut bytes);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(command: &str) -> Vec<String> {
        split(command).unwrap()
    }

    #[test]
    fn quotes_group_words_and_nothing_else_is_special() {
        assert_eq!(
            words(r#"git log --format="%h %s" 'a b'"#),
            ["git", "log", "--format=%h %s", "a b"]
        );
        assert_eq!(words("find ( -name x )"), ["find", "(", "-name", "x", ")"]);
    }

    #[test]
    fn what_a_shell_would_act_on_is_an_error() {
        for command in [
            "ls | wc",
            "cat a > b",
            "ls; rm x",
            "ls && rm x",
            "echo $HOME",
            "echo `id`",
            "ls\nrm x",
            "ls \\n",
        ] {
            assert_eq!(split(command).unwrap_err(), NO_SHELL, "{command}");
        }
        assert!(split("ls 'a").is_err());
        assert!(split("   ").is_err());
    }

    #[test]
    fn git_takes_no_abbreviation_of_a_denied_option() {
        for arg in [
            "--output=x",
            "--outp=x",
            "--ou=x",
            "--no-index",
            "--ext-diff",
            "--ext-d",
            "--dirty",
            "--dirty=-x",
            "--dir",
            "--broken",
            "--bro",
            "--b",
            "--di",
        ] {
            assert!(git_arguments(&[arg.to_string()]).is_err(), "{arg}");
        }
        assert!(git_arguments(&["--oneline".to_string(), "-n".to_string()]).is_ok());
    }

    #[test]
    fn a_command_that_runs_too_long_is_stopped() {
        let dir = std::env::temp_dir().join(format!("cb-bash-timeout-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fifo = dir.join("pipe");
        let made = Command::new("mkfifo").arg(&fifo).status().unwrap();
        assert!(made.success());
        // `cat` waits for a writer that never comes.
        let cmd = ["cat".to_string(), fifo.to_string_lossy().into_owned()];
        let started = Instant::now();
        let error = execute(&cmd, &dir, None, Duration::from_millis(300)).unwrap_err();
        assert!(error.ends_with("was stopped"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(5));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_more_than_the_cap_is_read_from_an_output() {
        assert_eq!(read_capped(std::io::repeat(b'a')).len() as u64, MAX_STREAM);
    }

    /// The words of `text`, with letters, digits and dashes inside a word.
    fn name_words(text: &str) -> Vec<&str> {
        text.split(|c: char| !(c.is_alphanumeric() || c == '-'))
            .filter(|word| !word.is_empty())
            .collect()
    }

    #[test]
    fn the_texts_name_every_program_and_git_subcommand_and_the_plan_runs_exactly_those() {
        use std::sync::Arc;

        use crate::chat::machine::{INSTRUCTION, machine_tools};
        use crate::workspace::Workspace;

        let dir = std::env::temp_dir().join(format!("cb-bash-text-{}", std::process::id()));
        let workspace = Arc::new(Workspace::open(&dir).unwrap());
        let tools = machine_tools(&workspace);
        let bash = tools.iter().find(|tool| tool.name() == "bash").unwrap();
        let description = name_words(bash.description());
        let instruction = name_words(INSTRUCTION);
        let folders = Folders::new(Arc::clone(&workspace));
        let vet = Vet {
            folders: &folders,
            directory: &dir,
            ceiling: None,
        };
        let refused = |parts: &[&str]| {
            let parts: Vec<String> = parts.iter().map(ToString::to_string).collect();
            vet.plan(&parts)
                .err()
                .is_some_and(|e| e.contains("is not allowed;"))
        };
        for program in ALLOWED.split(", ") {
            assert!(description.contains(&program), "{program}");
            assert!(instruction.contains(&program), "{program}");
            assert!(!refused(&[program]), "{program}");
        }
        assert!(refused(&["rm"]));
        for sub in GIT_SUBCOMMANDS {
            assert!(description.contains(&sub), "{sub}");
            assert!(!refused(&["git", sub]), "{sub}");
        }
        assert!(refused(&["git", "push"]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
