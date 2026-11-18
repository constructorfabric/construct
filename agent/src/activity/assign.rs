//! The project and the ticket of a session, by simple rules.
//!
//! * The project is the working folder without a `/.claude/worktrees/<name>` ending, with `\`
//!   read as `/`. A report names it by its last part, and by more of the path where two
//!   projects share a name.
//! * The ticket is the first id found in the name of the worktree folder, then in the branch
//!   name (unless it is a default branch or empty), then the id the user names most often in
//!   their messages of the session, where a tie gives none.
//!
//! The text of a message is read only by [`tickets_in_text`], which returns ids and nothing else.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;

/// What a report calls the sessions that have no ticket.
pub const NO_TICKET: &str = "No ticket";
/// What a report calls the sessions that have no working folder.
pub const NO_PROJECT: &str = "No project";

/// Branches that name no work.
const DEFAULT_BRANCHES: [&str; 4] = ["main", "master", "develop", "HEAD"];
/// Words that look like an id (`UTF-8`) and are not one.
const NOT_TICKETS: [&str; 8] = ["UTF", "SHA", "ISO", "RFC", "CVE", "TLS", "HTTP", "GPT"];
const WORKTREES: &str = "/.claude/worktrees/";

/// An id: 2 to 10 capital letters, a dash and up to 18 digits, as a whole word. So `PROJ-12-3`
/// holds `PROJ-12`, while `PROJ-1_2` and `foo_PROJ-1` hold none (`_` is part of a word), and a
/// longer number is no id (it would not fit a database key). The word ends are ASCII.
static ID: LazyLock<Regex> = LazyLock::new(|| {
    // The pattern is a constant, so it compiles or the first test fails.
    #[allow(clippy::unwrap_used)]
    Regex::new(r"(?-u:\b)([A-Z]{2,10})-[0-9]{1,18}(?-u:\b)").unwrap()
});

/// The folder with `\` read as `/`, without a trailing separator.
fn normalized(folder: &str) -> String {
    folder.replace('\\', "/").trim_end_matches('/').to_owned()
}

/// Where a worktree ending starts in a normalized path, and the name of the worktree.
fn worktree_ending(path: &str) -> Option<(usize, &str)> {
    let start = path.find(WORKTREES)?;
    let rest = &path[start + WORKTREES.len()..];
    let name = rest.split('/').next().filter(|n| !n.is_empty())?;
    Some((start, name))
}

/// The project of a working folder: the folder without a worktree ending, written with `/`.
#[must_use]
pub fn project_of(folder: &str) -> String {
    let path = normalized(folder);
    let project = worktree_ending(&path).map_or(path.as_str(), |(start, _)| &path[..start]);
    project.trim_end_matches('/').to_owned()
}

/// The name of the worktree folder a working folder is in, if it is in one.
#[must_use]
pub fn worktree_of(folder: &str) -> Option<String> {
    let path = normalized(folder);
    worktree_ending(&path).map(|(_, name)| name.to_owned())
}

/// What to call each of the projects: its last part, and for projects with one name as many
/// more parts as it takes to tell them apart; the whole path when that is not enough.
#[must_use]
pub fn project_labels<'a>(projects: impl IntoIterator<Item = &'a str>) -> BTreeMap<String, String> {
    let split = |path: &str| -> Vec<String> {
        path.split('/')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let suffix =
        |parts: &[String], depth: usize| parts[parts.len().saturating_sub(depth)..].join("/");
    let all: BTreeMap<&str, Vec<String>> = projects
        .into_iter()
        .map(|path| (path, split(path)))
        .collect();
    all.iter()
        .map(|(path, parts)| {
            let shown = if parts.is_empty() {
                NO_PROJECT.to_owned()
            } else {
                (1..=parts.len())
                    .map(|depth| suffix(parts, depth))
                    .find(|label| {
                        let depth = label.split('/').count();
                        all.iter()
                            .filter(|(other, _)| *other != path)
                            .all(|(_, theirs)| suffix(theirs, depth) != *label)
                    })
                    .unwrap_or_else(|| (*path).to_owned())
            };
            ((*path).to_owned(), shown)
        })
        .collect()
}

fn is_not_a_ticket(letters: &str) -> bool {
    NOT_TICKETS.contains(&letters)
}

/// Every id in a text, in the order they come. Returns the ids only; the text goes nowhere.
#[must_use]
pub fn tickets_in_text(text: &str) -> Vec<String> {
    ID.captures_iter(text)
        .filter(|found| !is_not_a_ticket(&found[1]))
        .map(|found| found[0].to_owned())
        .collect()
}

/// Every id a name (of a worktree folder or a branch) holds, in order.
#[must_use]
pub fn tickets_in_name(name: &str) -> Vec<String> {
    // In a name an underscore separates words, as a dash does.
    tickets_in_text(&name.replace('_', " "))
}

/// The ticket a name (of a worktree folder or a branch) holds: its first id.
fn ticket_in_name(name: &str) -> Option<String> {
    tickets_in_name(name).into_iter().next()
}

/// Whether a branch name can name work: it is not a default branch.
#[must_use]
pub fn branch_names_work(branch: &str) -> bool {
    !DEFAULT_BRANCHES.contains(&branch)
}

/// The ticket a branch names; none for a default branch or an empty name.
fn ticket_of_branch(branch: &str) -> Option<String> {
    if !branch_names_work(branch) {
        return None;
    }
    ticket_in_name(branch)
}

/// The id named most often, given how many times each is named; none when two share the lead.
#[must_use]
pub fn most_named(counts: &[(String, i64)]) -> Option<String> {
    let top = counts.iter().map(|(_, n)| *n).max().filter(|n| *n > 0)?;
    let mut leaders = counts.iter().filter(|(_, n)| *n == top);
    let (ticket, _) = leaders.next()?;
    leaders.next().is_none().then(|| ticket.clone())
}

/// The ticket of a session: its worktree folder, then its branch, then the id its user names
/// most often. Empty when none is found.
#[must_use]
pub fn ticket_for(folder: &str, branch: &str, named: &[(String, i64)]) -> String {
    worktree_of(folder)
        .and_then(|name| ticket_in_name(&name))
        .or_else(|| ticket_of_branch(branch))
        .or_else(|| most_named(named))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(pairs: &[(&str, i64)]) -> Vec<(String, i64)> {
        pairs.iter().map(|(t, n)| ((*t).to_owned(), *n)).collect()
    }

    #[test]
    fn the_project_is_the_folder_without_a_worktree_ending() {
        assert_eq!(project_of("/work/app"), "/work/app");
        assert_eq!(project_of("/work/app/"), "/work/app");
        assert_eq!(
            project_of("/work/app/.claude/worktrees/PROJ-1-fix"),
            "/work/app"
        );
        assert_eq!(
            project_of("/work/app/.claude/worktrees/PROJ-1-fix/src/lib"),
            "/work/app"
        );
        assert_eq!(project_of(""), "");
    }

    #[test]
    fn a_windows_path_reads_like_a_unix_one() {
        assert_eq!(
            project_of(r"C:\Users\me\app\.claude\worktrees\PROJ-7-x"),
            "C:/Users/me/app"
        );
        assert_eq!(
            worktree_of(r"C:\Users\me\app\.claude\worktrees\PROJ-7-x").as_deref(),
            Some("PROJ-7-x")
        );
        let labels = project_labels([project_of(r"C:\Users\me\app").as_str()]);
        assert_eq!(labels["C:/Users/me/app"], "app");
    }

    #[test]
    fn projects_with_one_name_show_enough_of_the_path() {
        let labels = project_labels(["/a/x/app", "/b/y/app", "/a/x/tool", "/c/z/app"]);
        assert_eq!(labels["/a/x/tool"], "tool");
        assert_eq!(labels["/a/x/app"], "x/app");
        assert_eq!(labels["/b/y/app"], "y/app");
        assert_eq!(labels["/c/z/app"], "z/app");
        let deep = project_labels(["/a/same/app", "/b/same/app"]);
        assert_eq!(deep["/a/same/app"], "a/same/app");
        assert_eq!(deep["/b/same/app"], "b/same/app");
        assert_eq!(project_labels([""])[""], NO_PROJECT);
        // A path that is the end of another one keeps the whole path, so the two differ.
        let ends = project_labels(["/x/app", "/a/x/app"]);
        assert_eq!(ends["/x/app"], "/x/app");
        assert_eq!(ends["/a/x/app"], "a/x/app");
    }

    #[test]
    fn a_worktree_folder_gives_its_ticket() {
        assert_eq!(
            ticket_for("/w/app/.claude/worktrees/PROJ-12-fix-it", "master", &[]),
            "PROJ-12"
        );
        assert_eq!(
            ticket_for("/w/app/.claude/worktrees/fix-PROJ-9", "PROJ-77", &[]),
            "PROJ-9"
        );
        // The worktree wins over the branch and the text.
        assert_eq!(
            ticket_for(
                "/w/app/.claude/worktrees/AB-1-x",
                "CD-2",
                &named(&[("EF-3", 9)])
            ),
            "AB-1"
        );
    }

    #[test]
    fn a_branch_that_names_no_work_gives_nothing_and_the_text_decides() {
        for branch in ["main", "master", "develop", "HEAD", ""] {
            assert_eq!(ticket_of_branch(branch), None, "{branch}");
            assert_eq!(
                ticket_for("/w/app", branch, &named(&[("EF-3", 2), ("GH-4", 1)])),
                "EF-3"
            );
        }
        assert_eq!(ticket_for("/w/app", "feature/PROJ-5-x", &[]), "PROJ-5");
        assert_eq!(ticket_for("/w/app", "feature-PROJ-5", &[]), "PROJ-5");
        assert_eq!(
            ticket_for("/w/app", "feature/AB-12_fix-login", &[]),
            "AB-12"
        );
        assert_eq!(ticket_for("/w/app", "master", &[]), "");
    }

    #[test]
    fn names_in_lower_case_hold_no_ticket() {
        for name in [
            "feature-123",
            "fix-1",
            "release-2026-01",
            "node-18.x",
            "proj-5-x",
        ] {
            assert_eq!(ticket_in_name(name), None, "{name}");
        }
        assert_eq!(
            ticket_for("/w/app/.claude/worktrees/feature-123", "AB-4", &[]),
            "AB-4"
        );
    }

    #[test]
    fn a_tie_gives_none() {
        assert_eq!(most_named(&named(&[("AB-1", 2), ("CD-2", 2)])), None);
        assert_eq!(
            most_named(&named(&[("AB-1", 3), ("CD-2", 2)])).as_deref(),
            Some("AB-1")
        );
        assert_eq!(most_named(&[]), None);
    }

    #[test]
    fn words_that_are_not_tickets_give_none() {
        for text in [
            "UTF-8",
            "SHA-256",
            "CVE-2026-1234",
            "see TLS-1 and HTTP-2 for GPT-4",
        ] {
            assert!(tickets_in_text(text).is_empty(), "{text}");
        }
        assert_eq!(ticket_of_branch("UTF-8-fix"), None);
        assert_eq!(
            ticket_for("/w/.claude/worktrees/SHA-256-x", "ISO-1", &[]),
            ""
        );
    }

    #[test]
    fn an_id_is_a_whole_word_of_capitals() {
        assert_eq!(
            tickets_in_text("Fix AB-12, then (CD-3) and AB-12 again."),
            vec!["AB-12", "CD-3", "AB-12"]
        );
        assert!(tickets_in_text("ab-12 A-1 ABCDEFGHIJK-1 xAB-1 AB-1x AB- AB-x").is_empty());
        assert_eq!(tickets_in_text("ABCDEFGHIJ-10"), vec!["ABCDEFGHIJ-10"]);
        // The ends of a word: a dash, a dot or a bracket end it, an underscore does not.
        assert_eq!(
            tickets_in_text("PROJ-12-3 PROJ-1.5 [PROJ-2]"),
            vec!["PROJ-12", "PROJ-1", "PROJ-2"]
        );
        assert!(tickets_in_text("PROJ-1_2 foo_PROJ-1").is_empty());
        // A long number stays whole, and a letter after it is not a word end.
        let longest = format!("AB-{}", "7".repeat(18));
        assert_eq!(tickets_in_text(&longest), vec![longest.clone()]);
        assert!(tickets_in_text(&format!("{longest}7")).is_empty());
        assert!(tickets_in_text(&format!("AB-{}", "7".repeat(5000))).is_empty());
        assert!(tickets_in_text(&format!("{longest}x")).is_empty());
        // The word ends are ASCII: text glued to an id in a script without spaces still has it.
        assert_eq!(tickets_in_text("修复AB-123问题"), vec!["AB-123"]);
    }
}
