//! What the setup session found out about the user's work, as far as it changes how a session is
//! assigned: which folder belongs to which project of which tracker, and the groupings that suit
//! their work. With nothing found, a session is assigned by the plain rules of [`assign`].
//!
//! * An id still has to look like an id to [`assign`] (capital letters, a dash, digits, and not a
//!   word such as `ISO`). Of those, a session in a folder tied to a tracker takes the ids whose
//!   key is a project of that tracker; an id whose key belongs to another tracker is left out.
//! * A folder is tied to a project by a name in the `folders` of a mapping, or, when a mapping
//!   names no folder, by the name of the repository (the first such mapping wins). A name matches
//!   the end of the folder path, whole parts only, so a subfolder of a tied folder is not tied.
//! * A session belongs to the first grouping that lists its project.

use std::collections::BTreeSet;

use serde_json::Value;

use super::assign;

/// What a report calls the sessions that belong to no grouping.
pub const NO_GROUP: &str = "No group";

/// A project of a tracker and the folders that belong to it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mapping {
    pub tracker: String,
    pub project: String,
    pub repository: String,
    pub folders: Vec<String>,
}

/// A named set of projects that belong together: the work of one client, one course, one topic.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Collection {
    pub name: String,
    pub projects: Vec<String>,
}

/// What the setup found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Findings {
    mappings: Vec<Mapping>,
    collections: Vec<Collection>,
}

/// What the findings say about one session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Assigned {
    /// The project of the session: its working folder without a worktree ending.
    pub project: String,
    /// The ticket, empty when the session has none.
    pub ticket: String,
    /// The tracker the session works with, empty when the findings do not say.
    pub tracker: String,
    /// What a report calls the project of the tracker the folder is tied to, empty when it is
    /// tied to none.
    pub area: String,
    /// The grouping the session belongs to, empty when it belongs to none.
    pub collection: String,
}

fn text(value: &Value, member: &str) -> String {
    value
        .get(member)
        .and_then(Value::as_str)
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

fn list(value: &Value, member: &str) -> Vec<String> {
    value
        .get(member)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// A path with `\` read as `/`, without a trailing separator.
fn normalized(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_owned()
}

/// Whether `name` is the end of `path`, whole parts only: `shop` and `team/shop` end
/// `/w/team/shop`, `hop` does not.
fn ends_with_parts(path: &str, name: &str) -> bool {
    let name = normalized(name);
    let name = name.trim_start_matches('/');
    !name.is_empty()
        && (path == name
            || path
                .strip_suffix(name)
                .is_some_and(|rest| rest.ends_with('/')))
}

/// The name of a repository in its address: `git@host:team/shop.git` is `shop`.
fn repository_name(address: &str) -> &str {
    let name = address.trim_end_matches('/');
    let name = name.rsplit(['/', ':']).next().unwrap_or(name);
    name.strip_suffix(".git").unwrap_or(name)
}

impl Findings {
    /// The findings in the text of `findings.json`; nothing when it cannot be read.
    #[must_use]
    pub fn parse(source: &str) -> Self {
        Self::try_parse(source).unwrap_or_default()
    }

    /// The findings in the text of `findings.json`, or `None` when it is not a JSON object, for
    /// example a file that is half written.
    #[must_use]
    pub fn try_parse(source: &str) -> Option<Self> {
        let root = serde_json::from_str::<Value>(source).ok()?;
        root.is_object().then_some(())?;
        let items = |member: &str| -> Vec<Value> {
            root.get(member)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        };
        let mappings = items("mappings")
            .iter()
            .map(|m| Mapping {
                tracker: text(m, "tracker"),
                project: text(m, "project"),
                repository: text(m, "repository"),
                folders: list(m, "folders"),
            })
            // A mapping with no project ties nothing to a tracker.
            .filter(|m| !m.project.is_empty())
            .collect();
        let collections = items("groupings")
            .iter()
            .map(|g| Collection {
                name: text(g, "name"),
                projects: list(g, "projects"),
            })
            .filter(|g| !g.name.is_empty())
            .collect();
        Some(Self {
            mappings,
            collections,
        })
    }

    /// Whether the findings say nothing that changes an assignment.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty() && self.collections.is_empty()
    }

    /// The mapping a folder (as [`assign::project_of`] gives it) is tied to. A named folder beats
    /// a repository name, and a longer name beats a shorter one.
    fn mapping_of(&self, project: &str) -> Option<&Mapping> {
        let path = normalized(project);
        let mut best: Option<(&Mapping, usize)> = None;
        for mapping in &self.mappings {
            for folder in &mapping.folders {
                let len = normalized(folder).len();
                if ends_with_parts(&path, folder) && best.is_none_or(|(_, longest)| len > longest) {
                    best = Some((mapping, len));
                }
            }
        }
        if let Some((mapping, _)) = best {
            return Some(mapping);
        }
        let name = path.rsplit('/').next().filter(|n| !n.is_empty())?;
        self.mappings
            .iter()
            .find(|m| m.folders.is_empty() && repository_name(&m.repository) == name)
    }

    /// The trackers that have `prefix` as a project key.
    fn trackers_of_key(&self, prefix: &str) -> BTreeSet<&str> {
        self.mappings
            .iter()
            .filter(|m| m.project.eq_ignore_ascii_case(prefix))
            .map(|m| m.tracker.as_str())
            .collect()
    }

    /// The tracker that is the only one with a project of the key of `id`; empty when there is
    /// none or several.
    fn sole_owner(&self, id: &str) -> String {
        let owners = self.trackers_of_key(id.split('-').next().unwrap_or_default());
        match owners.iter().next() {
            Some(only) if owners.len() == 1 => (*only).to_owned(),
            _ => String::new(),
        }
    }

    /// Whether an id belongs to the tracker the session works with, or, when its folder is tied
    /// to none, to any tracker the findings know.
    fn accepts(&self, tracker: Option<&str>, id: &str) -> bool {
        let owners = self.trackers_of_key(id.split('-').next().unwrap_or_default());
        match tracker {
            Some(tracker) => owners.contains(tracker),
            None => !owners.is_empty(),
        }
    }

    /// Whether no tracker of the findings has a project with the key of the id.
    fn unclaimed(&self, id: &str) -> bool {
        self.trackers_of_key(id.split('-').next().unwrap_or_default())
            .is_empty()
    }

    /// What a report calls a ticket: the id, and the tracker after it when two trackers have a
    /// project with the key of the id. A saved filter names the ticket as the report shows it.
    fn qualified(&self, tracker: &str, id: &str) -> String {
        let prefix = id.split('-').next().unwrap_or_default();
        if tracker.is_empty() || self.trackers_of_key(prefix).len() < 2 {
            id.to_owned()
        } else {
            format!("{id} ({tracker})")
        }
    }

    /// What a report calls a project of a tracker: its key, and the tracker after it when two
    /// trackers have a project with that key, so the two are not one row.
    fn area_label(&self, tracker: &str, project: &str) -> String {
        if self.trackers_of_key(project).len() < 2 {
            project.to_owned()
        } else {
            format!("{project} ({tracker})")
        }
    }

    /// The project, the ticket, the tracker and the grouping of a session, from its working
    /// folder, its branch and how often its user named each id.
    #[must_use]
    pub fn assign(&self, folder: &str, branch: &str, named: &[(String, i64)]) -> Assigned {
        let project = assign::project_of(folder);
        if self.mappings.is_empty() {
            return Assigned {
                ticket: assign::ticket_for(folder, branch, named),
                collection: self.collection_of(&project, ""),
                project,
                ..Assigned::default()
            };
        }
        let mapping = self.mapping_of(&project);
        let tracker = mapping.map(|m| m.tracker.as_str());
        // The ids of the tracker first; then the ids no tracker has a project for, which the
        // findings say nothing about, so the plain rules take them as they are.
        let chain = |keep: &dyn Fn(&str) -> bool| {
            let from_name = |name: &str| {
                assign::tickets_in_name(name)
                    .into_iter()
                    .find(|id| keep(id))
            };
            assign::worktree_of(folder)
                .and_then(|name| from_name(&name))
                .or_else(|| {
                    assign::branch_names_work(branch)
                        .then(|| from_name(branch))
                        .flatten()
                })
                .or_else(|| {
                    let counts: Vec<(String, i64)> =
                        named.iter().filter(|(id, _)| keep(id)).cloned().collect();
                    assign::most_named(&counts)
                })
        };
        let ticket = chain(&|id| self.accepts(tracker, id))
            .or_else(|| chain(&|id| self.unclaimed(id)))
            .unwrap_or_default();
        let tracker = mapping.map_or_else(|| self.sole_owner(&ticket), |m| m.tracker.clone());
        let key = mapping.map(|m| m.project.clone()).unwrap_or_default();
        let collection = self.collection_of(&project, &key);
        let area = if key.is_empty() {
            key
        } else {
            self.area_label(&tracker, &key)
        };
        let ticket = if ticket.is_empty() {
            ticket
        } else {
            self.qualified(&tracker, &ticket)
        };
        Assigned {
            project,
            ticket,
            tracker,
            area,
            collection,
        }
    }

    /// The first grouping that lists the project of the tracker or the last part of the folder.
    fn collection_of(&self, project: &str, area: &str) -> String {
        let last = normalized(project);
        let last = last.rsplit('/').next().unwrap_or_default();
        self.collections
            .iter()
            .find(|c| {
                c.projects
                    .iter()
                    .any(|p| (!area.is_empty() && p == area) || (!last.is_empty() && p == last))
            })
            .map(|c| c.name.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = r#"{
        "trackers": [{"name": "alpha"}, {"name": "beta"}],
        "mappings": [
            {"tracker": "alpha", "project": "PLAT", "repository": "git@h:t/shop.git", "folders": ["shop"]},
            {"tracker": "beta", "project": "BLOG", "repository": "git@h:t/blog.git", "folders": []},
            {"tracker": "", "project": "", "repository": "git@h:t/lab.git", "folders": ["lab"]}
        ],
        "groupings": [{"name": "work", "projects": ["PLAT", "BLOG"]}]
    }"#;

    #[test]
    fn a_mapping_without_a_project_and_a_file_that_is_not_json_say_nothing() {
        let found = Findings::parse(TEXT);
        assert_eq!(found.mappings.len(), 2);
        assert!(Findings::parse("not json").is_empty());
        assert!(Findings::parse(r#"{"mappings": "x", "groupings": [1]}"#).is_empty());
    }

    #[test]
    fn a_folder_matches_whole_parts_and_a_repository_name_is_the_fallback() {
        let found = Findings::parse(TEXT);
        assert_eq!(found.mapping_of("/w/shop").unwrap().project, "PLAT");
        assert_eq!(found.mapping_of("/w/team/shop").unwrap().project, "PLAT");
        assert!(found.mapping_of("/w/workshop").is_none());
        assert_eq!(found.mapping_of("/w/blog").unwrap().project, "BLOG");
        assert!(found.mapping_of("/w/lab").is_none());
        assert!(found.mapping_of("").is_none());
    }

    #[test]
    fn the_longest_folder_name_wins() {
        let found = Findings::parse(
            r#"{"mappings": [
                {"tracker": "a", "project": "ONE", "folders": ["app"]},
                {"tracker": "b", "project": "TWO", "folders": ["client/app"]}
            ]}"#,
        );
        assert_eq!(found.mapping_of("/w/client/app").unwrap().project, "TWO");
        assert_eq!(found.mapping_of("/w/other/app").unwrap().project, "ONE");
    }

    #[test]
    fn two_trackers_with_one_key_are_told_apart_by_the_folder() {
        let found = Findings::parse(
            r#"{"mappings": [
                {"tracker": "alpha", "project": "WEB", "folders": ["a-site"]},
                {"tracker": "beta", "project": "WEB", "folders": ["b-site"]}
            ]}"#,
        );
        assert_eq!(
            found.assign("/w/a-site", "WEB-4", &[]).ticket,
            "WEB-4 (alpha)"
        );
        assert_eq!(
            found.assign("/w/b-site", "WEB-4", &[]).ticket,
            "WEB-4 (beta)"
        );
        // The projects are told apart too, so two organizations are two rows.
        assert_eq!(found.assign("/w/a-site", "", &[]).area, "WEB (alpha)");
        assert_eq!(found.assign("/w/b-site", "", &[]).area, "WEB (beta)");
    }

    #[test]
    fn a_tied_session_leaves_the_ids_of_another_tracker() {
        let found = Findings::parse(TEXT);
        // The shop is alpha's and its branch names only an id of beta's: no ticket.
        assert_eq!(found.assign("/w/shop", "BLOG-3-x", &[]).ticket, "");
        // A key no tracker has is no claim, and the plain rules take the id.
        assert_eq!(found.assign("/w/shop", "ZZ-1-x", &[]).ticket, "ZZ-1");
        // A folder tied to none takes the id of any tracker.
        assert_eq!(found.assign("/w/other", "BLOG-3-x", &[]).ticket, "BLOG-3");
        assert_eq!(found.assign("/w/other", "BLOG-3-x", &[]).tracker, "beta");
    }

    #[test]
    fn the_first_grouping_that_lists_a_project_wins() {
        let found = Findings::parse(
            r#"{"groupings": [
                {"name": "one", "projects": ["lab"]},
                {"name": "two", "projects": ["lab", "shop"]}
            ]}"#,
        );
        // Groupings alone, with no mapping, still group by the name of the folder.
        assert_eq!(found.assign("/w/lab", "", &[]).collection, "one");
        assert_eq!(found.assign("/w/shop", "", &[]).collection, "two");
        assert_eq!(found.assign("/w/blog", "", &[]).collection, "");
    }

    #[test]
    fn a_half_written_file_is_not_findings() {
        assert!(Findings::try_parse("{\"mappings\": [").is_none());
        assert!(Findings::try_parse("[]").is_none());
        assert!(Findings::try_parse("{}").is_some());
    }
}
