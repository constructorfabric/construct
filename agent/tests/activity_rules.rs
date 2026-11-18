//! The time rules on hand-made sessions: no database, no clock, no time zone.

#![cfg(unix)]
// Test code: a failed `unwrap` is a failed test.
#![allow(clippy::unwrap_used)]

use construct_agent::activity::assign::project_of;
use construct_agent::activity::rules::{
    Day, DaysError, Grouping, Kind, PARTS_PER_MINUTE, Report, SessionMinutes, calculate,
    minute_number,
};

fn at(time: &str) -> i64 {
    minute_number(time).unwrap()
}

/// `count` minutes in a row from `start`.
fn run(start: &str, count: i64) -> Vec<i64> {
    (at(start)..at(start) + count).collect()
}

fn session(name: &str, folder: &str, unattended: bool, minutes: Vec<i64>) -> SessionMinutes {
    SessionMinutes {
        tool: "claude".into(),
        session: name.into(),
        project: project_of(folder),
        ticket: String::new(),
        unattended,
        minutes,
    }
}

/// A day from midnight to midnight, UTC.
fn utc_day(date: &str, next: &str) -> Day {
    Day {
        label: date.into(),
        start: at(&format!("{date}T00:00")),
        end: at(&format!("{next}T00:00")),
    }
}

fn nov_11() -> Vec<Day> {
    vec![utc_day("2026-11-11", "2026-11-12")]
}

fn total(sessions: &[SessionMinutes], kind: Kind) -> u64 {
    calculate(sessions, &nov_11(), kind, Grouping::Project)
        .unwrap()
        .total_parts
}

fn whole(minutes: i64) -> u64 {
    u64::try_from(minutes).unwrap() * PARTS_PER_MINUTE
}

fn assert_adds_up(report: &Report) {
    let sum: u64 = report.groups.iter().map(|g| g.parts).sum();
    assert_eq!(sum, report.total_parts, "{report:?}");
}

#[test]
fn ten_minutes_in_a_row_give_ten() {
    let sessions = [session("a", "/x", false, run("2026-11-11T10:00", 10))];
    assert_eq!(total(&sessions, Kind::Both), whole(10));
}

#[test]
fn a_single_active_minute_counts_one_minute() {
    let sessions = [session("a", "/x", false, run("2026-11-11T10:00", 1))];
    assert_eq!(total(&sessions, Kind::Both), whole(1));
}

#[test]
fn a_pause_of_five_counts_and_a_pause_of_six_does_not() {
    let mut five = run("2026-11-11T10:00", 1);
    five.extend(run("2026-11-11T10:06", 1));
    // The silent minutes 10:01 to 10:05 are part of the block: 7 minutes in all.
    assert_eq!(
        total(&[session("a", "/x", false, five)], Kind::Both),
        whole(7)
    );

    let mut six = run("2026-11-11T10:00", 1);
    six.extend(run("2026-11-11T10:07", 1));
    // Two blocks of one minute each.
    assert_eq!(
        total(&[session("a", "/x", false, six)], Kind::Both),
        whole(2)
    );
}

#[test]
fn the_order_and_repeats_of_the_minutes_do_not_matter() {
    let mut minutes = run("2026-11-11T10:00", 3);
    minutes.reverse();
    minutes.extend(run("2026-11-11T10:01", 2));
    assert_eq!(
        total(&[session("a", "/x", false, minutes)], Kind::Both),
        whole(3)
    );
}

#[test]
fn two_sessions_in_the_same_minutes_split_each_minute_in_halves() {
    let sessions = [
        session("a", "/x", false, run("2026-11-11T10:00", 10)),
        session("b", "/y", false, run("2026-11-11T10:00", 10)),
    ];
    let report = calculate(&sessions, &nov_11(), Kind::Both, Grouping::Project).unwrap();
    // The clock time is 10 minutes, not 20.
    assert_eq!(report.total_parts, whole(10));
    assert_eq!(report.sessions, 2);
    assert_eq!(report.groups.len(), 2);
    for group in &report.groups {
        assert_eq!(group.parts, whole(5));
        assert!((group.share - 0.5).abs() < f64::EPSILON);
        assert_eq!(group.sessions, 1);
    }
    assert_eq!(report.groups[0].key, "x");
    assert_adds_up(&report);
}

#[test]
fn a_partial_overlap_shares_only_the_minutes_both_are_active_in() {
    let sessions = [
        session("a", "/x", false, run("2026-11-11T10:00", 4)),
        session("b", "/y", false, run("2026-11-11T10:02", 4)),
    ];
    let report = calculate(&sessions, &nov_11(), Kind::Both, Grouping::Project).unwrap();
    // 10:00-10:05 is 6 minutes; 10:02 and 10:03 are shared.
    assert_eq!(report.total_parts, whole(6));
    assert_eq!(report.groups[0].parts, whole(3));
    assert_eq!(report.groups[1].parts, whole(3));
}

#[test]
fn a_minute_shared_three_ways_adds_up_exactly() {
    let sessions = ["a", "b", "c"].map(|n| session(n, n, false, run("2026-11-11T10:00", 1)));
    let report = calculate(&sessions, &nov_11(), Kind::Both, Grouping::Project).unwrap();
    assert_eq!(report.total_parts, whole(1));
    assert_adds_up(&report);
    let parts: Vec<u64> = report.groups.iter().map(|g| g.parts).collect();
    // The remainder goes to the first group in order.
    assert_eq!(parts, vec![333_334, 333_333, 333_333]);
    // Asked again, the answer is the same.
    assert_eq!(
        report,
        calculate(&sessions, &nov_11(), Kind::Both, Grouping::Project).unwrap()
    );
}

#[test]
fn a_minute_is_shared_between_projects_not_between_sessions() {
    // Two sessions in one project and one in another, all active in the same minute: each
    // project has half, however many sessions it has.
    let sessions = [
        session("a", "/x", false, run("2026-11-11T10:00", 1)),
        session("b", "/x", false, run("2026-11-11T10:00", 1)),
        session("c", "/y", false, run("2026-11-11T10:00", 1)),
    ];
    let report = calculate(&sessions, &nov_11(), Kind::Both, Grouping::Project).unwrap();
    let parts: Vec<(&str, u64, usize)> = report
        .groups
        .iter()
        .map(|g| (g.key.as_str(), g.parts, g.sessions))
        .collect();
    assert_eq!(
        parts,
        vec![
            ("x", PARTS_PER_MINUTE / 2, 2),
            ("y", PARTS_PER_MINUTE / 2, 1)
        ]
    );
    assert_eq!(report.sessions, 3);
    // Grouped by tool, all three are one group and have the whole minute.
    let by_tool = calculate(&sessions, &nov_11(), Kind::Both, Grouping::Tool).unwrap();
    assert_eq!(by_tool.groups.len(), 1);
    assert_eq!(by_tool.groups[0].parts, PARTS_PER_MINUTE);
}

#[test]
fn an_unattended_run_beside_the_users_session_counts_as_the_users() {
    let sessions = [
        session("own", "/x", false, run("2026-11-11T10:00", 10)),
        session("script", "/y", true, run("2026-11-11T10:00", 20)),
    ];
    let both = calculate(&sessions, &nov_11(), Kind::Both, Grouping::Project).unwrap();
    assert_eq!(both.total_parts, whole(20));
    let by_key = |r: &Report, key: &str| r.groups.iter().find(|g| g.key == key).unwrap().parts;
    // The shared ten minutes all go to the user's session; the script has the last ten.
    assert_eq!(by_key(&both, "x"), whole(10));
    assert_eq!(by_key(&both, "y"), whole(10));

    let own = calculate(&sessions, &nov_11(), Kind::Person, Grouping::Project).unwrap();
    assert_eq!((own.total_parts, own.groups.len()), (whole(10), 1));
    let unattended = calculate(&sessions, &nov_11(), Kind::Unattended, Grouping::Project).unwrap();
    assert_eq!(unattended.total_parts, whole(10));
    assert_eq!(unattended.groups[0].key, "y");
    // Own and unattended together make both.
    assert_eq!(own.total_parts + unattended.total_parts, both.total_parts);
}

#[test]
fn a_block_over_midnight_is_cut_at_the_boundary_the_test_passes_in() {
    // 23:55 to 00:04, ten minutes.
    let sessions = [session("a", "/x", false, run("2026-11-11T23:55", 10))];
    let days = [
        utc_day("2026-11-11", "2026-11-12"),
        utc_day("2026-11-12", "2026-11-13"),
    ];
    let report = calculate(&sessions, &days, Kind::Both, Grouping::Day).unwrap();
    let parts: Vec<(&str, u64)> = report
        .groups
        .iter()
        .map(|g| (g.key.as_str(), g.parts))
        .collect();
    assert_eq!(
        parts,
        vec![("2026-11-11", whole(5)), ("2026-11-12", whole(5))]
    );
    assert_eq!(report.total_parts, whole(10));

    // The same session in a period of the first day only: the rest is outside.
    assert_eq!(total(&sessions, Kind::Both), whole(5));

    // The caller's own midnight, two hours ahead of UTC, cuts elsewhere: the day starts at
    // 22:00 UTC, so 23:55 UTC is 5 minutes after the cut of the day before.
    let local = [
        Day {
            label: "2026-11-11".into(),
            start: at("2026-11-10T22:00"),
            end: at("2026-11-11T22:00"),
        },
        Day {
            label: "2026-11-12".into(),
            start: at("2026-11-11T22:00"),
            end: at("2026-11-12T22:00"),
        },
    ];
    let report = calculate(&sessions, &local, Kind::Both, Grouping::Day).unwrap();
    assert_eq!(report.groups[0].parts, 0);
    assert_eq!(report.groups[1].parts, whole(10));
}

#[test]
fn a_pause_over_midnight_is_still_a_block_and_is_cut() {
    let mut minutes = run("2026-11-11T23:58", 2);
    minutes.extend(run("2026-11-12T00:03", 1));
    let sessions = [session("a", "/x", false, minutes)];
    let days = [
        utc_day("2026-11-11", "2026-11-12"),
        utc_day("2026-11-12", "2026-11-13"),
    ];
    let report = calculate(&sessions, &days, Kind::Both, Grouping::Day).unwrap();
    assert_eq!(report.groups[0].parts, whole(2));
    assert_eq!(report.groups[1].parts, whole(4));
    assert_eq!(report.sessions, 1);
    assert_eq!(report.groups.iter().map(|g| g.sessions).sum::<usize>(), 2);
}

#[test]
fn a_day_of_23_or_25_hours_gives_the_right_total() {
    // The session is active round the clock for three days.
    let sessions = [session(
        "a",
        "/x",
        false,
        run("2026-03-28T00:00", 3 * 24 * 60),
    )];
    // The clocks go forward: the day is 23 hours, then back: the next is 25.
    let short = Day {
        label: "short".into(),
        start: at("2026-03-28T23:00"),
        end: at("2026-03-29T22:00"),
    };
    let long = Day {
        label: "long".into(),
        start: at("2026-03-29T22:00"),
        end: at("2026-03-30T23:00"),
    };
    let report = calculate(&sessions, &[short, long], Kind::Both, Grouping::Day).unwrap();
    assert_eq!(report.groups[0].parts, whole(23 * 60));
    assert_eq!(report.groups[1].parts, whole(25 * 60));
    assert_eq!(report.total_parts, whole(48 * 60));
    assert_adds_up(&report);
}

#[test]
fn the_groups_add_up_to_the_total_whatever_the_grouping() {
    let mut a = session("a", "/x", false, run("2026-11-11T09:58", 6));
    a.ticket = "PROJ-1".into();
    let mut b = session("b", "/y", true, run("2026-11-11T10:00", 9));
    b.ticket = "PROJ-2".into();
    let mut c = session("c", "/x", false, run("2026-11-11T10:01", 3));
    c.tool = "codex".into();
    let sessions = [a, b, c];
    for kind in [Kind::Person, Kind::Unattended, Kind::Both] {
        for grouping in [
            Grouping::Project,
            Grouping::Ticket,
            Grouping::Tool,
            Grouping::Day,
        ] {
            let report = calculate(&sessions, &nov_11(), kind, grouping).unwrap();
            assert_adds_up(&report);
            let share: f64 = report.groups.iter().map(|g| g.share).sum();
            assert!(
                report.total_parts == 0 || (share - 1.0).abs() < 1e-9,
                "{share}"
            );
        }
    }
    let by_ticket = calculate(&sessions, &nov_11(), Kind::Both, Grouping::Ticket).unwrap();
    let keys: Vec<&str> = by_ticket.groups.iter().map(|g| g.key.as_str()).collect();
    assert!(keys.contains(&"PROJ-1") && keys.contains(&"PROJ-2") && keys.contains(&"No ticket"));
    let by_tool = calculate(&sessions, &nov_11(), Kind::Both, Grouping::Tool).unwrap();
    assert_eq!(by_tool.groups.len(), 2);
}

#[test]
fn a_period_without_time_gives_an_empty_report() {
    let report = calculate(&[], &nov_11(), Kind::Both, Grouping::Project).unwrap();
    assert_eq!(
        (report.total_parts, report.sessions, report.groups.len()),
        (0, 0, 0)
    );
    let days = calculate(&[], &nov_11(), Kind::Both, Grouping::Day).unwrap();
    assert_eq!((days.groups.len(), days.groups[0].share), (1, 0.0));
}

#[test]
fn days_that_end_before_they_start_or_overlap_are_refused() {
    let empty = Day {
        label: "d".into(),
        start: 10,
        end: 10,
    };
    assert!(calculate(&[], &[empty], Kind::Both, Grouping::Day).is_err());
    let one = Day {
        label: "one".into(),
        start: 0,
        end: 10,
    };
    let two = Day {
        label: "two".into(),
        start: 9,
        end: 20,
    };
    let error: DaysError = calculate(&[], &[two, one], Kind::Both, Grouping::Day).unwrap_err();
    assert_eq!(error.to_string(), "the days one and two overlap");
    let again = Day {
        label: "one".into(),
        start: 20,
        end: 30,
    };
    let one = Day {
        label: "one".into(),
        start: 0,
        end: 10,
    };
    assert!(calculate(&[], &[one, again], Kind::Both, Grouping::Day).is_err());
}

#[test]
fn projects_are_the_folders_without_a_worktree_ending_and_named_by_their_last_parts() {
    let sessions = [
        session("a", "/work/app", false, run("2026-11-11T10:00", 2)),
        session(
            "b",
            "/work/app/.claude/worktrees/PROJ-1-fix",
            false,
            run("2026-11-11T11:00", 3),
        ),
        session(
            "c",
            r"C:\code\tool\.claude\worktrees\x",
            false,
            run("2026-11-11T12:00", 4),
        ),
        session("d", "/other/app", false, run("2026-11-11T13:00", 5)),
    ];
    let report = calculate(&sessions, &nov_11(), Kind::Both, Grouping::Project).unwrap();
    let groups: Vec<(&str, u64)> = report
        .groups
        .iter()
        .map(|g| (g.key.as_str(), g.parts))
        .collect();
    // The worktree is part of its project; the two folders called `app` are told apart.
    assert_eq!(
        groups,
        [
            ("other/app", whole(5)),
            ("work/app", whole(5)),
            ("tool", whole(4))
        ]
    );
}

#[test]
fn tickets_group_by_the_stored_value_and_sessions_without_one_are_called_so() {
    let mut a = session("a", "/x", false, run("2026-11-11T10:00", 2));
    a.ticket = "AB-1".into();
    let b = session("b", "/x", false, run("2026-11-11T11:00", 3));
    let mut c = session("c", "/y", false, run("2026-11-11T12:00", 4));
    c.ticket = "AB-1".into();
    let report = calculate(&[a, b, c], &nov_11(), Kind::Both, Grouping::Ticket).unwrap();
    let groups: Vec<(&str, u64, usize)> = report
        .groups
        .iter()
        .map(|g| (g.key.as_str(), g.parts, g.sessions))
        .collect();
    assert_eq!(groups, [("AB-1", whole(6), 2), ("No ticket", whole(3), 1)]);
}
