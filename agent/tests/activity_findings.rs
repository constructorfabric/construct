//! Assigning sessions by the findings of the setup: fixture sessions in three repositories of two
//! trackers. The first part asks the rules directly; the second runs the store against the
//! embedded PostgreSQL and is ignored by default: `cargo test -- --ignored`.

#![cfg(unix)]
// Test code: a failed `unwrap` is a failed test.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

mod common;

use common::sessions::{at, run, session};
use construct_agent::activity::assign;
use construct_agent::activity::findings::{Assigned, Findings, NO_GROUP};
use construct_agent::activity::rules::{
    Day, Grouping, Kind, PARTS_PER_MINUTE, SessionMinutes, calculate,
};
use construct_agent::dashboard::Filter;
use construct_agent::dashboard::default_activity;
use construct_agent::dashboard_numbers::{Spec, numbers, respond};
use serde_json::{Value, json};

/// Two trackers, in different organizations, and three repositories: the shop and the blog are
/// tied to a project each, the lab to none. The shop and the blog make the group `work`.
const FINDINGS: &str = r#"{
    "trackers": [
        { "name": "alpha", "kind": "issue tracker", "evidence": "PLAT-12 in the history" },
        { "name": "beta", "kind": "issue tracker", "evidence": "BLOG-3 in the history" }
    ],
    "mappings": [
        { "tracker": "alpha", "project": "PLAT", "repository": "git@git.example.test:team/shop.git",
          "folders": ["shop"], "important": true },
        { "tracker": "beta", "project": "BLOG", "repository": "git@git.example.test:team/blog.git",
          "folders": ["blog"], "important": true },
        { "tracker": "", "project": "", "repository": "git@git.example.test:team/lab.git",
          "folders": ["lab"], "important": false }
    ],
    "groupings": [{ "name": "work", "projects": ["PLAT", "BLOG"] }]
}"#;

fn parse(source: &str) -> Findings {
    Findings::try_parse(source).unwrap()
}

/// What a session is: its folder, its branch and the ids its user named.
struct Place {
    name: &'static str,
    folder: &'static str,
    branch: &'static str,
    named: &'static [(&'static str, i64)],
}

fn places() -> Vec<Place> {
    vec![
        // The branch names a ticket of the other tracker; the folder says which tracker counts.
        Place {
            name: "shop",
            folder: "/w/shop",
            branch: "feature/BLOG-3-copy",
            named: &[("PLAT-12", 2)],
        },
        Place {
            name: "shop-worktree",
            folder: "/w/shop/.claude/worktrees/PLAT-20-cart",
            branch: "main",
            named: &[],
        },
        Place {
            name: "blog",
            folder: "/w/blog",
            branch: "BLOG-3-post",
            named: &[("PLAT-12", 5)],
        },
        Place {
            name: "lab",
            folder: "/w/lab",
            branch: "LAB-1-try",
            named: &[],
        },
    ]
}

/// The session the time rules take, as the findings assigned it: ten minutes from `start`.
fn in_report(name: &str, assigned: &Assigned, start: i64) -> SessionMinutes {
    let mut built = session(
        "claude",
        name,
        &assigned.project,
        &assigned.ticket,
        &run(start, 10),
    );
    built.area.clone_from(&assigned.area);
    built.collection.clone_from(&assigned.collection);
    built
}

fn one_day() -> Day {
    Day {
        label: "2026-11-10".into(),
        start: at("2026-11-10T00:00"),
        end: at("2026-11-11T00:00"),
    }
}

fn named(place: &Place) -> Vec<(String, i64)> {
    place
        .named
        .iter()
        .map(|(id, n)| ((*id).to_owned(), *n))
        .collect()
}

fn assign_all(findings: &Findings) -> BTreeMap<&'static str, Assigned> {
    places()
        .iter()
        .map(|p| (p.name, findings.assign(p.folder, p.branch, &named(p))))
        .collect()
}

#[test]
fn each_session_lands_on_the_ticket_of_its_own_tracker() {
    let got = assign_all(&parse(FINDINGS));
    // The shop is alpha's: the id of the blog's tracker in its branch is left, the id its user
    // names is taken.
    assert_eq!(got["shop"].ticket, "PLAT-12");
    assert_eq!(got["shop-worktree"].ticket, "PLAT-20");
    assert_eq!(got["shop-worktree"].project, "/w/shop");
    // The blog is beta's: its branch gives the ticket, and the id of alpha's that its user names
    // more often does not take its place.
    assert_eq!(got["blog"].ticket, "BLOG-3");
}

#[test]
fn a_folder_tied_to_a_project_is_grouped_under_it() {
    let got = assign_all(&parse(FINDINGS));
    assert_eq!(got["shop"].area, "PLAT");
    assert_eq!(got["shop-worktree"].area, "PLAT");
    assert_eq!(got["blog"].area, "BLOG");
    assert_eq!(got["shop"].collection, "work");
    assert_eq!(got["blog"].collection, "work");

    // Both shop folders are one group in a report by project, the lab keeps its own name.
    let sessions: Vec<SessionMinutes> = places()
        .iter()
        .zip(0..)
        .map(|(p, i)| in_report(p.name, &got[p.name], at("2026-11-10T08:00") + i * 60))
        .collect();
    let day = one_day();
    let keys = |grouping| -> Vec<(String, u64)> {
        let report =
            calculate(&sessions, std::slice::from_ref(&day), Kind::Both, grouping).unwrap();
        let mut groups: Vec<_> = report
            .groups
            .into_iter()
            .map(|g| (g.key, g.parts))
            .collect();
        groups.sort();
        groups
    };
    let ten = 10 * PARTS_PER_MINUTE;
    assert_eq!(
        keys(Grouping::Project),
        [
            ("BLOG".to_owned(), ten),
            ("PLAT".to_owned(), 2 * ten),
            ("lab".to_owned(), ten)
        ]
    );
    assert_eq!(
        keys(Grouping::Collection),
        [(NO_GROUP.to_owned(), ten), ("work".to_owned(), 3 * ten)]
    );
}

#[test]
fn with_no_findings_the_plain_rules_decide() {
    let none = Findings::default();
    for place in places() {
        let got = none.assign(place.folder, place.branch, &named(&place));
        assert_eq!(
            got.project,
            assign::project_of(place.folder),
            "{}",
            place.name
        );
        assert_eq!(
            got.ticket,
            assign::ticket_for(place.folder, place.branch, &named(&place)),
            "{}",
            place.name
        );
        assert_eq!((got.area.as_str(), got.collection.as_str()), ("", ""));
    }
    // A file that says nothing assigns as no file does.
    assert_eq!(assign_all(&parse("{}")), assign_all(&none));
    // The lab is tied to no project, so the findings leave it to the plain rules.
    let with = assign_all(&parse(FINDINGS));
    assert_eq!(with["lab"], assign_all(&none)["lab"]);
    assert_eq!(with["lab"].ticket, "LAB-1");
}

#[test]
fn a_name_the_user_gave_still_holds_when_the_findings_name_the_project() {
    let found = parse(FINDINGS);
    let a = found.assign("/w/shop", "main", &[]);
    let session = in_report("s", &a, at("2026-11-10T08:00"));
    let day = one_day();
    let ask = |hidden: &[&str], names: &[(&str, &str)]| {
        numbers(
            std::slice::from_ref(&session),
            std::slice::from_ref(&day),
            &Spec {
                kind: Some(Kind::Person),
                grouping: Some(Grouping::Project),
                filter: Filter::default(),
                hidden_projects: hidden.iter().map(|s| (*s).to_owned()).collect(),
                project_names: names
                    .iter()
                    .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                    .collect(),
            },
        )
        .unwrap()
    };
    // Without a correction the page names the project by the findings.
    assert_eq!(ask(&[], &[]).groups[0].key, "PLAT");
    // A rename made by the folder, by the old name or by the new one wins over the findings.
    for key in ["/w/shop", "shop", "PLAT"] {
        assert_eq!(ask(&[], &[(key, "Store")]).groups[0].key, "Store", "{key}");
    }
    // So does hiding, whichever way the project was named.
    for key in ["/w/shop", "shop", "PLAT"] {
        let answer = ask(&[key], &[]);
        assert!(answer.groups.is_empty(), "{key}");
        assert_eq!(answer.hidden_projects, 1, "{key}");
    }
}

#[test]
fn two_trackers_with_one_project_key_are_two_rows_in_a_report() {
    let found = parse(
        r#"{"mappings": [
            {"tracker": "alpha", "project": "WEB", "folders": ["site-a"]},
            {"tracker": "beta", "project": "WEB", "folders": ["site-b"]}
        ]}"#,
    );
    let sessions: Vec<SessionMinutes> = [("a", "/w/site-a"), ("b", "/w/site-b")]
        .iter()
        .zip(0..)
        .map(|((name, folder), i)| {
            let assigned = found.assign(folder, "WEB-4", &[]);
            in_report(name, &assigned, at("2026-11-10T08:00") + i * 60)
        })
        .collect();
    let day = one_day();
    let report = calculate(&sessions, &[day], Kind::Both, Grouping::Project).unwrap();
    let mut keys: Vec<_> = report.groups.iter().map(|g| g.key.as_str()).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["WEB (alpha)", "WEB (beta)"]);
}

#[test]
fn two_folders_tied_to_one_project_are_one_group_and_the_groupings_only_case_groups() {
    let found = parse(
        r#"{"mappings": [
            {"tracker": "alpha", "project": "PLAT", "folders": ["shop", "shop-old"]}
        ],
        "groupings": [{"name": "one", "projects": ["PLAT"]}, {"name": "two", "projects": ["PLAT"]}]}"#,
    );
    let sessions: Vec<SessionMinutes> = [("a", "/w/shop"), ("b", "/w/shop-old")]
        .iter()
        .zip(0..)
        .map(|((name, folder), i)| {
            in_report(
                name,
                &found.assign(folder, "main", &[]),
                at("2026-11-10T08:00") + i * 60,
            )
        })
        .collect();
    assert_ne!(sessions[0].project, sessions[1].project);
    let day = one_day();
    let by = |grouping| {
        let report = calculate(&sessions, std::slice::from_ref(&day), Kind::Both, grouping);
        let report = report.unwrap();
        (report.groups.len(), report.groups[0].key.clone())
    };
    assert_eq!(by(Grouping::Project), (1, "PLAT".to_owned()));
    // The first grouping that lists the project wins.
    assert_eq!(by(Grouping::Collection), (1, "one".to_owned()));

    // Groupings with no mapping group by the name of the folder.
    let only = parse(r#"{"groupings": [{"name": "clients", "projects": ["lab"]}]}"#);
    assert_eq!(only.assign("/w/lab", "", &[]).collection, "clients");
}

#[test]
fn a_widget_that_groups_by_collection_is_answered_through_the_page_request() {
    let found = parse(FINDINGS);
    let sessions: Vec<SessionMinutes> = places()
        .iter()
        .zip(0..)
        .map(|(p, i)| {
            let assigned = found.assign(p.folder, p.branch, &named(p));
            in_report(p.name, &assigned, at("2026-11-10T08:00") + i * 60)
        })
        .collect();
    let mut document = default_activity();
    let table = document["payload"]["widgets"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|w| w["id"] == "by-project")
        .unwrap();
    table["group_by"] = json!("collection");
    let id = "by-project";
    let request = json!({
        "widget": id,
        "days": [{ "label": "2026-11-10", "start": "2026-11-10T00:00:00Z", "end": "2026-11-11T00:00:00Z" }],
    })
    .to_string();
    let reply: Value =
        serde_json::from_str(&respond(&request, Some(&document), || Ok(sessions.clone()))).unwrap();
    let keys: Vec<&str> = reply["groups"]
        .as_array()
        .unwrap_or_else(|| panic!("no groups in {reply}"))
        .iter()
        .map(|g| g["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["work", NO_GROUP]);
}

#[test]
fn the_collection_grouping_is_asked_for_by_the_page() {
    let found = parse(FINDINGS);
    let sessions: Vec<SessionMinutes> = places()
        .iter()
        .zip(0..)
        .map(|(p, i)| {
            let assigned = found.assign(p.folder, p.branch, &named(p));
            in_report(p.name, &assigned, at("2026-11-10T08:00") + i * 60)
        })
        .collect();
    let answer = numbers(
        &sessions,
        &[one_day()],
        &Spec {
            kind: Some(Kind::Person),
            grouping: Some(Grouping::Collection),
            filter: Filter::default(),
            hidden_projects: Vec::new(),
            project_names: BTreeMap::new(),
        },
    )
    .unwrap();
    let rows: Vec<(&str, f64)> = answer
        .groups
        .iter()
        .map(|g| (g.key.as_str(), g.minutes))
        .collect();
    assert_eq!(rows, [("work", 30.0), (NO_GROUP, 10.0)]);
}

#[cfg(target_os = "linux")]
mod store {
    use super::*;
    use connectors::{Author, Record, Role, Turn};
    use construct_agent::activity::{RULES_VERSION, Session};
    use construct_agent::records::{FileSink, Sink};

    use crate::common::pg_stand::PgStand as Stand;

    /// One user message at `minute` past 08:00 of the day, from `place`.
    fn said(place: &Place, minute: i64) -> Record {
        Record::Turn(Turn {
            tool: "claude".into(),
            session: place.name.into(),
            time: format!("2026-11-10T08:{minute:02}:00Z"),
            folder: place.folder.into(),
            branch: place.branch.into(),
            role: Role::User,
            author: Author::Person,
            text: place
                .named
                .iter()
                .map(|(id, _)| format!("about {id}"))
                .collect::<Vec<_>>()
                .join(" "),
            tools: Vec::new(),
        })
    }

    fn sink(stand: &Stand) -> FileSink {
        let mut sink = FileSink::new(&stand.root.join("records-dir"));
        for (i, place) in places().iter().enumerate() {
            for minute in 0..3 {
                sink.append(&said(place, i64::try_from(i).unwrap() * 10 + minute))
                    .unwrap();
            }
        }
        sink
    }

    type Done = Box<dyn Fn(&BTreeMap<String, Session>) -> bool>;

    fn by_name(sessions: &[Session]) -> BTreeMap<String, Session> {
        sessions
            .iter()
            .map(|s| (s.session.clone(), s.clone()))
            .collect()
    }

    async fn reports(store: &construct_agent::activity::Store) -> Vec<Vec<(String, u64)>> {
        let day = Day {
            label: "2026-11-10".into(),
            start: at("2026-11-10T00:00"),
            end: at("2026-11-11T00:00"),
        };
        let mut all = Vec::new();
        for grouping in [Grouping::Project, Grouping::Ticket, Grouping::Tool] {
            let report = store
                .activity(std::slice::from_ref(&day), Kind::Both, grouping)
                .await
                .unwrap();
            all.push(
                report
                    .groups
                    .into_iter()
                    .map(|g| (g.key, g.parts))
                    .collect(),
            );
        }
        all
    }

    #[tokio::test]
    #[ignore = "starts PostgreSQL; run with --ignored"]
    async fn the_store_assigns_by_the_findings_and_with_none_gives_the_plain_numbers() {
        let stand = Stand::new("act-findings").await;
        let store = stand.store().await;
        let sink = sink(&stand);
        store.rebuild(&sink).await.unwrap();
        let plain = reports(&store).await;
        let plain_sessions = store.sessions().await.unwrap();

        // The findings tell the sessions apart: the shop gets its own tracker's ticket.
        let changed = store.set_findings(parse(FINDINGS)).await.unwrap();
        assert_eq!(
            changed,
            [
                ("claude".to_owned(), "blog".to_owned()),
                ("claude".to_owned(), "shop".to_owned()),
                ("claude".to_owned(), "shop-worktree".to_owned())
            ]
        );
        let found = by_name(&store.sessions().await.unwrap());
        assert_eq!(found["shop"].ticket, "PLAT-12");
        assert_eq!(found["blog"].ticket, "BLOG-3");
        assert_eq!(found["shop-worktree"].area, "PLAT");
        assert_eq!(found["lab"].area, "");

        // A rebuild from the files under the same findings gives the same sessions.
        let with_findings = store.sessions().await.unwrap();
        store.rebuild(&sink).await.unwrap();
        assert_eq!(store.sessions().await.unwrap(), with_findings);

        // The same findings again change nothing; none at all give back the plain numbers.
        assert!(
            store
                .set_findings(parse(FINDINGS))
                .await
                .unwrap()
                .is_empty()
        );
        let undone = store.set_findings(Findings::default()).await.unwrap();
        assert_eq!(undone.len(), 3);
        assert_eq!(store.sessions().await.unwrap(), plain_sessions);
        assert_eq!(reports(&store).await, plain);
        store.close().await;
        stand.finish().await;
    }

    #[tokio::test]
    #[ignore = "starts PostgreSQL; run with --ignored"]
    async fn a_changed_mapping_assigns_again_only_the_sessions_it_touches() {
        let stand = Stand::new("act-findings-touch").await;
        let store = stand.store().await;
        store.rebuild(&sink(&stand)).await.unwrap();
        store.set_findings(parse(FINDINGS)).await.unwrap();
        let before = by_name(&store.sessions().await.unwrap());

        // The lab now belongs to the blog's project.
        let moved = FINDINGS.replace(
            r#""tracker": "", "project": "", "repository": "git@git.example.test:team/lab.git""#,
            r#""tracker": "beta", "project": "BLOG", "repository": "git@git.example.test:team/lab.git""#,
        );
        assert_ne!(moved, FINDINGS);
        let changed = store.set_findings(parse(&moved)).await.unwrap();
        assert_eq!(changed, [("claude".to_owned(), "lab".to_owned())]);
        let after = by_name(&store.sessions().await.unwrap());
        assert_eq!(after["lab"].area, "BLOG");
        assert_eq!(after["lab"].collection, "work");
        for name in ["shop", "shop-worktree", "blog"] {
            assert_eq!(after[name], before[name], "{name}");
        }
        store.close().await;
        stand.finish().await;
    }

    #[tokio::test]
    #[ignore = "starts PostgreSQL; run with --ignored"]
    async fn a_store_of_the_older_rules_is_made_again() {
        let stand = Stand::new("act-findings-version").await;
        let store = stand.store().await;
        store.rebuild(&sink(&stand)).await.unwrap();
        store.close().await;

        // The store as the older rules made it: no columns for the findings.
        let mut conn = stand.connect().await;
        for statement in [
            "update activity.rules set version = 2",
            "alter table activity.sessions drop column area",
            "alter table activity.sessions drop column collection",
        ] {
            sqlx::query(statement).execute(&mut conn).await.unwrap();
        }
        drop(conn);

        let store = stand.store().await;
        let version: i32 = sqlx::query_scalar("select version from activity.rules")
            .fetch_one(&mut stand.connect().await)
            .await
            .unwrap();
        assert_eq!(version, RULES_VERSION);
        const { assert!(RULES_VERSION > 2) };
        assert!(store.sessions().await.unwrap().is_empty());
        store.rebuild(&sink(&stand)).await.unwrap();
        assert_eq!(store.sessions().await.unwrap().len(), 4);
        store.close().await;
        stand.finish().await;
    }

    /// The feed reads `findings.json` from the workspace: at the start, when it changes, and not
    /// when it is half written; a record that comes in live is assigned by what it read.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "starts PostgreSQL; run with --ignored"]
    async fn the_feed_follows_the_file_of_the_findings() {
        use construct_agent::activity::{FINDINGS_POLL, Feed};
        use construct_agent::workspace::Workspace;
        use std::sync::Arc;
        use std::time::{Duration, Instant};

        let stand = Stand::new("act-findings-feed").await;
        let store = Arc::new(stand.store().await);
        let workspace = Arc::new(Workspace::open(&stand.root.join("data")).unwrap());
        workspace.write("findings.json", FINDINGS).unwrap();
        let feed = Feed::new();
        feed.watch_findings(Arc::clone(&workspace));
        let files = sink(&stand);
        let task = {
            let (feed, store) = (feed.clone(), Arc::clone(&store));
            tokio::spawn(async move { feed.run(&store, &files).await })
        };
        let wait = |what: &'static str, done: Done| {
            let store = Arc::clone(&store);
            async move {
                let start = Instant::now();
                loop {
                    if done(&by_name(&store.sessions().await.unwrap())) {
                        return;
                    }
                    assert!(start.elapsed() < Duration::from_secs(30), "{what}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        };

        // The file as it was at the start is applied to the sessions built from the records.
        wait(
            "the findings were not read at the start",
            Box::new(|s| {
                s.get("shop")
                    .is_some_and(|s| s.ticket == "PLAT-12" && s.area == "PLAT")
            }),
        )
        .await;

        // A record that comes in live lands on its tracker's ticket.
        let live = Place {
            name: "shop-live",
            folder: "/w/shop",
            branch: "feature/BLOG-3-copy",
            named: &[("PLAT-30", 1)],
        };
        feed.recorder()(&said(&live, 40));
        wait(
            "a live record was not assigned by the findings",
            Box::new(|s| s.get("shop-live").is_some_and(|s| s.ticket == "PLAT-30")),
        )
        .await;

        // A change of the file moves the lab, and only the lab.
        let moved = FINDINGS.replace(
            r#""tracker": "", "project": "", "repository": "git@git.example.test:team/lab.git""#,
            r#""tracker": "beta", "project": "BLOG", "repository": "git@git.example.test:team/lab.git""#,
        );
        workspace.write("findings.json", &moved).unwrap();
        wait(
            "the changed findings were not read",
            Box::new(|s| s.get("lab").is_some_and(|s| s.area == "BLOG")),
        )
        .await;

        // A half written file leaves what was read in place.
        workspace
            .write("findings.json", "{\"mappings\": [")
            .unwrap();
        // Two more reads of the file, so that one began after the write.
        let reads = feed.findings_reads();
        let limit = Instant::now() + 6 * FINDINGS_POLL;
        while feed.findings_reads() < reads + 2 {
            assert!(Instant::now() < limit, "the feed did not read the file");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let kept = by_name(&store.sessions().await.unwrap());
        assert_eq!(kept["lab"].area, "BLOG");
        assert_eq!(kept["shop"].area, "PLAT");

        // A file that is gone means no findings.
        workspace.remove_all("findings.json").unwrap();
        wait(
            "a missing file was not read as no findings",
            Box::new(|s| s.get("shop").is_some_and(|s| s.area.is_empty())),
        )
        .await;

        task.abort();
        let _ = task.await;
        store.close().await;
        stand.finish().await;
    }
}
