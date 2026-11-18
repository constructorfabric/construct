//! The time rules: how many minutes of work a period holds, for a kind of session and a grouping.
//!
//! This is the only place with these rules; a page or the chat asks it and never counts on its
//! own. It is a plain function of its input, with no clock, no time zone, no model and no
//! network: the same sessions and days always give the same report.
//!
//! * A minute is active when a session has a turn or a mark in it.
//! * A pause of up to [`MAX_PAUSE`] silent minutes between two active minutes of a session is
//!   part of the work; a longer pause ends the block. A block counts from its first to its last
//!   active minute, so it is at least one minute.
//! * Sessions run on one timeline. A minute is counted once, however many sessions are active
//!   in it, and is shared equally between the groups of those sessions: two sessions of one project
//!   are one share, not two.
//! * In a shared minute the sessions of the user win over unattended ones: the unattended
//!   sessions get no part of it.
//! * The caller says where each day starts and ends. A block that crosses the end of a day is
//!   cut there, and a minute outside every day is not counted.
//!
//! Minutes are whole numbers of minutes since the Unix epoch (UTC). Time is split into
//! millionths of a minute, so a shared minute adds up exactly: the remainder of a division goes
//! to the first groups in order, and the groups always add up to the total.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use connectors::minute_of;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::assign;

/// The longest pause, in silent minutes, that still joins two active minutes into one block.
pub const MAX_PAUSE: i64 = 5;

/// How far, in minutes, a minute of a session can be from a day and still change what the day
/// holds: a pause of [`MAX_PAUSE`] silent minutes joins two active minutes into one block, so a
/// block that reaches into a day has an active minute within this distance of it. A caller that
/// keeps only the minutes near its days keeps every block whole where it matters.
pub const REACH: i64 = MAX_PAUSE + 1;

/// How many parts a minute is split into.
pub const PARTS_PER_MINUTE: u64 = 1_000_000;

/// Whose work a report counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The sessions of the user only.
    Person,
    /// The sessions that ran unattended, in the minutes the user had no session of their own.
    Unattended,
    /// Both: the minutes of [`Kind::Person`] and of [`Kind::Unattended`] together.
    Both,
}

/// What a report is split by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grouping {
    /// The project of a session: its working folder without a worktree ending, named by its
    /// last parts (see [`assign::project_labels`]).
    Project,
    /// The ticket of a session, by the rules in [`assign`]; sessions without one are grouped
    /// as [`assign::NO_TICKET`].
    Ticket,
    /// The tool that wrote a session.
    Tool,
    /// The ticket of a session; a session without one is grouped by its project, as
    /// `No ticket (project)`. This is the grouping of the summary of a day.
    TicketOrProject,
    /// The days the caller gave, oldest first.
    Day,
}

/// One session, as the store keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionMinutes {
    pub tool: String,
    pub session: String,
    /// The project kept in the store: the working folder without a worktree ending.
    pub project: String,
    /// The ticket kept in the store, empty when the session has none.
    pub ticket: String,
    /// True when every prompt of the session came from a script. A session with a prompt by a
    /// person, or by an author that is not known, counts as the user's own.
    pub unattended: bool,
    /// The active minutes, in any order, as returned by [`minute_number`].
    pub minutes: Vec<i64>,
}

/// One day of the period, as the caller sees it: it starts at `start` and ends before `end`.
/// A day of 23 or 25 hours, when the clocks change, is only a different `end - start`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Day {
    /// What the day is called in a report grouped by day, for example `2026-11-11`.
    pub label: String,
    pub start: i64,
    pub end: i64,
}

/// What a report holds for one group.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// The project, the ticket, the tool or the label of the day.
    pub key: String,
    /// The time of the group, in [`PARTS_PER_MINUTE`]th of a minute.
    pub parts: u64,
    /// The share of the total, from 0 to 1; 0 when the total is 0.
    pub share: f64,
    /// The sessions that have time in the group.
    pub sessions: usize,
}

/// The time one session has in one group: the minutes in which it counted there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// The index of the session in the input.
    pub session: usize,
    /// The first minute it counted in, since the epoch.
    pub first: i64,
    /// The last minute it counted in.
    pub last: i64,
    /// How many whole minutes it counted in; a minute shared with other groups counts whole.
    pub minutes: u64,
}

/// The answer: the groups, their total and the sessions that have any time in the period.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    /// By time, the longest first, ties by key; for [`Grouping::Day`] in the order of the days,
    /// with a group for every day, also an empty one.
    pub groups: Vec<Group>,
    /// The clock time counted, in [`PARTS_PER_MINUTE`]th of a minute. It equals the sum of the
    /// groups.
    pub total_parts: u64,
    pub sessions: usize,
}

/// The days are not usable.
#[derive(Debug, PartialEq, Eq)]
pub struct DaysError(String);

impl fmt::Display for DaysError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DaysError {}

/// The minute a UTC time (`2026-11-11T10:15:30Z`, or a minute as `2026-11-11T10:15`) falls in,
/// counted in minutes since the Unix epoch. `None` when it is not a real date and time.
#[must_use]
pub fn minute_number(time: &str) -> Option<i64> {
    let minute = minute_of(time);
    // A time with an offset (`+02:00`) is not UTC and is not read as if it were.
    if minute.len() != 16
        || (time.len() > 16 && !time.ends_with('Z'))
        || minute.as_bytes().get(10) != Some(&b'T')
    {
        return None;
    }
    let parsed = OffsetDateTime::parse(&format!("{minute}:00Z"), &Rfc3339).ok()?;
    Some(parsed.unix_timestamp().div_euclid(60))
}

/// Whether a session counts as unattended: it has prompts from scripts and none from a person
/// or from an author that is not known.
#[must_use]
pub fn is_unattended(person_prompts: i64, unattended_prompts: i64, unknown_prompts: i64) -> bool {
    unattended_prompts > 0 && person_prompts == 0 && unknown_prompts == 0
}

/// The blocks of one session, as `(first, last)` minutes, both counted.
fn blocks(minutes: &[i64]) -> Vec<(i64, i64)> {
    let mut sorted = minutes.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut blocks: Vec<(i64, i64)> = Vec::new();
    for minute in sorted {
        match blocks.last_mut() {
            Some((_, last)) if minute - *last - 1 <= MAX_PAUSE => *last = minute,
            _ => blocks.push((minute, minute)),
        }
    }
    blocks
}

/// Whether the days can be used: each ends after it starts, and none repeats a label or overlaps
/// another. [`calculate`] checks them too; a caller that reads its sessions first checks before.
///
/// # Errors
/// Fails when a day is empty, a label is given twice, or two days overlap.
pub fn check_days(days: &[Day]) -> Result<(), DaysError> {
    let mut days = days.to_vec();
    days.sort_by_key(|d| d.start);
    check_sorted(&days)
}

fn check_sorted(days: &[Day]) -> Result<(), DaysError> {
    if let Some(day) = days.iter().find(|d| d.start >= d.end) {
        return Err(DaysError(format!(
            "the day {} ends before it starts",
            day.label
        )));
    }
    let mut labels = BTreeSet::new();
    if let Some(day) = days.iter().find(|d| !labels.insert(d.label.as_str())) {
        return Err(DaysError(format!("the day {} is given twice", day.label)));
    }
    if let Some(pair) = days.windows(2).find(|w| w[0].end > w[1].start) {
        return Err(DaysError(format!(
            "the days {} and {} overlap",
            pair[0].label, pair[1].label
        )));
    }
    Ok(())
}

/// The time of the sessions in the days, of one kind and split by one grouping.
///
/// The days may come in any order but must not overlap, and each must end after it starts.
///
/// # Errors
/// Fails when a day is empty or two days overlap.
pub fn calculate(
    sessions: &[SessionMinutes],
    days: &[Day],
    kind: Kind,
    grouping: Grouping,
) -> Result<Report, DaysError> {
    // Named over all the sessions given, so that a project keeps its name whatever the period.
    let names = assign::project_labels(sessions.iter().map(|s| s.project.as_str()));
    calculate_named(sessions, days, kind, grouping, &names)
}

/// [`calculate`] with the name of each project given by the caller, by the project the store
/// keeps: a report grouped by project has one group for each name, so projects given one name are
/// one group and share a minute as one. `names` must hold every project of `sessions`.
///
/// # Errors
/// Fails when a day is empty or two days overlap.
pub fn calculate_named(
    sessions: &[SessionMinutes],
    days: &[Day],
    kind: Kind,
    grouping: Grouping,
    names: &BTreeMap<String, String>,
) -> Result<Report, DaysError> {
    calculate_spans(sessions, days, kind, grouping, names).map(|(report, _)| report)
}

/// [`calculate_named`] with, for each group, the sessions that have time in it and when: the
/// sessions of a group are as many as `Group::sessions` says.
///
/// # Errors
/// Fails when a day is empty or two days overlap.
// The pass is one loop over the timeline, and its steps share the running state of the open
// sessions, so cutting it up would only pass that state around.
#[allow(clippy::too_many_lines)]
pub fn calculate_spans(
    sessions: &[SessionMinutes],
    days: &[Day],
    kind: Kind,
    grouping: Grouping,
    names: &BTreeMap<String, String>,
) -> Result<(Report, BTreeMap<String, Vec<Span>>), DaysError> {
    let mut days = days.to_vec();
    days.sort_by_key(|d| d.start);
    check_sorted(&days)?;

    // The minutes of every session on one timeline, each with the sessions active in it, in the
    // order of the input.
    let mut timeline: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (index, session) in sessions.iter().enumerate() {
        for (first, last) in blocks(&session.minutes) {
            for minute in first..=last {
                timeline.entry(minute).or_default().push(index);
            }
        }
    }

    // group -> (position, parts, sessions with time in it)
    let mut groups: BTreeMap<String, (usize, u64, BTreeMap<usize, Span>)> = BTreeMap::new();
    if grouping == Grouping::Day {
        for (position, day) in days.iter().enumerate() {
            groups.insert(day.label.clone(), (position, 0, BTreeMap::new()));
        }
    }
    let mut total_parts = 0;
    let mut counted_sessions = BTreeSet::new();
    for (minute, active) in &timeline {
        let after = days.partition_point(|d| d.start <= *minute);
        let Some(day) = after
            .checked_sub(1)
            .map(|i| &days[i])
            .filter(|d| *minute < d.end)
        else {
            continue;
        };
        let own: Vec<usize> = active
            .iter()
            .copied()
            .filter(|&i| !sessions[i].unattended)
            .collect();
        let winners = match (kind, own.is_empty()) {
            (Kind::Person | Kind::Both, false) => own,
            (Kind::Unattended | Kind::Both, true) => active.clone(),
            _ => continue,
        };
        // The sessions that count in this minute, by the group each belongs to.
        let mut by_key: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for index in winners {
            let project = || names[&sessions[index].project].clone();
            let key = match grouping {
                Grouping::Project => project(),
                Grouping::TicketOrProject if sessions[index].ticket.is_empty() => {
                    format!("{} ({})", assign::NO_TICKET, project())
                }
                Grouping::Ticket if sessions[index].ticket.is_empty() => {
                    assign::NO_TICKET.to_owned()
                }
                Grouping::Ticket | Grouping::TicketOrProject => sessions[index].ticket.clone(),
                Grouping::Tool => sessions[index].tool.clone(),
                Grouping::Day => day.label.clone(),
            };
            by_key.entry(key).or_default().push(index);
        }
        // The minute is shared equally between the groups, so a group with more sessions in
        // it does not get more.
        let share_count = by_key.len() as u64;
        for (rank, (key, members)) in by_key.into_iter().enumerate() {
            let parts = PARTS_PER_MINUTE / share_count
                + u64::from((rank as u64) < PARTS_PER_MINUTE % share_count);
            let entry = groups.entry(key).or_insert((0, 0, BTreeMap::new()));
            entry.1 += parts;
            total_parts += parts;
            counted_sessions.extend(members.iter().copied());
            for session in members {
                let span = entry.2.entry(session).or_insert(Span {
                    session,
                    first: *minute,
                    last: *minute,
                    minutes: 0,
                });
                span.last = *minute;
                span.minutes += 1;
            }
        }
    }

    let mut spans: BTreeMap<String, Vec<Span>> = BTreeMap::new();
    let mut groups: Vec<(usize, Group)> = groups
        .into_iter()
        .map(|(key, (position, parts, members))| {
            spans.insert(key.clone(), members.values().copied().collect());
            #[allow(clippy::cast_precision_loss)] // a total never nears 2^52 parts
            let share = if total_parts == 0 {
                0.0
            } else {
                parts as f64 / total_parts as f64
            };
            (
                position,
                Group {
                    key,
                    parts,
                    share,
                    sessions: members.len(),
                },
            )
        })
        .collect();
    if grouping == Grouping::Day {
        groups.sort_by_key(|(position, _)| *position);
    } else {
        groups.sort_by(|(_, a), (_, b)| b.parts.cmp(&a.parts).then_with(|| a.key.cmp(&b.key)));
    }
    let report = Report {
        groups: groups.into_iter().map(|(_, group)| group).collect(),
        total_parts,
        sessions: counted_sessions.len(),
    };
    Ok((report, spans))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_are_counted_from_the_epoch() {
        assert_eq!(minute_number("1970-01-01T00:01:59Z"), Some(1));
        assert_eq!(minute_number("1969-12-31T23:59"), Some(-1));
        assert_eq!(
            minute_number("2026-11-11T10:15:30.5Z"),
            minute_number("2026-11-11T10:15")
        );
        assert_eq!(minute_number("2026-11-11T10:15:30+02:00"), None);
        assert_eq!(minute_number("2026-13-11T10:15"), None);
        assert_eq!(minute_number("2026-11-11 10:15"), None);
        assert_eq!(minute_number("soon"), None);
    }

    #[test]
    fn only_scripts_make_a_session_unattended() {
        assert!(is_unattended(0, 3, 0));
        assert!(!is_unattended(1, 3, 0));
        assert!(!is_unattended(0, 3, 1));
        assert!(!is_unattended(0, 0, 0));
    }
}
