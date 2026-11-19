//! The numbers of the On screen source: how long each application, site, project and ticket was
//! in front, as the dashboard shows it.
//!
//! [`numbers`] answers in the shape [`crate::dashboard_numbers::numbers`] uses for the sessions
//! (groups with minutes and shares, the total, the minutes of each day), so a page draws both the
//! same way, and a chat tool that answers the same question calls it too. It works from the
//! blocks the store keeps (see [`crate::activity::screen_time`]): one application in front at a
//! time, no overlap, no title and no address.
//!
//! * A block counts in the days it overlaps, by the second: one that crosses a midnight is split
//!   there, so the groups always add up to the total, and the blocks of a group add up to its
//!   number ([`blocks_of`]).
//! * The hidden projects and the names of the page apply as they do to the sessions. A block
//!   names its project by the last part of the folder, and the sessions of the store tell which
//!   folders have that name: a name that two folders share is the page's name for them only when
//!   both are shown by it, else the name of the block as it is.
//! * A block with no site, no project or no ticket is a group of its own ([`NO_SITE`],
//!   [`NO_PROJECT`], [`assign::NO_TICKET`]), so nothing is left out of a grouping.
//! * A ticket filter keeps the blocks of the tickets it lists, a project filter those of the
//!   projects it names. A filter by tools has nothing to match and is refused before this runs.

use std::collections::{BTreeMap, BTreeSet};

use crate::activity::assign::NO_TICKET;
use crate::activity::rules::{Day, DaysError, SessionMinutes, check_days};
use crate::activity::screen_time::{Block, epoch, project_name};
use crate::dashboard_numbers::{Answer, Names, Row, Spec, names_of};

/// The group of the blocks that are not on a site.
pub const NO_SITE: &str = "No site";
/// The group of the blocks that name no project.
pub const NO_PROJECT: &str = "No project";
/// The group of the blocks of an application with no name.
pub const NO_APP: &str = "Unknown app";

/// What the time on screen is split by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenGrouping {
    Project,
    Ticket,
    App,
    Site,
    /// The days the caller gave, oldest first.
    Day,
}

/// The part of one block that falls in one day.
struct Piece<'a> {
    block: usize,
    source: &'a Block,
    /// The project as the page shows it.
    project: String,
    day: usize,
    /// Seconds since the epoch, `from` counted and `to` not.
    from: i64,
    to: i64,
}

impl Piece<'_> {
    fn seconds(&self) -> u64 {
        u64::try_from(self.to - self.from).unwrap_or(0)
    }
}

/// The pieces of the blocks that stay, and how many hidden projects have time in the days.
struct Selected<'a> {
    pieces: Vec<Piece<'a>>,
    hidden_projects: usize,
}

/// How the blocks' project names are shown, and which of them a list names.
struct Projects<'a> {
    names: Names,
    /// The folders that have a name, by that name.
    folders: BTreeMap<String, Vec<String>>,
    spec: &'a Spec,
}

impl Projects<'_> {
    fn new<'a>(sessions: &[SessionMinutes], spec: &'a Spec) -> Projects<'a> {
        let names = names_of(sessions, spec);
        let mut folders: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for folder in names.labels.keys() {
            folders
                .entry(project_name(folder).to_owned())
                .or_default()
                .push(folder.clone());
        }
        Projects {
            names,
            folders,
            spec,
        }
    }

    /// The name the page shows for the project of a block.
    fn shown(&self, project: &str) -> String {
        let Some(folders) = self.folders.get(project) else {
            return self
                .spec
                .project_names
                .get(project)
                .map_or(project, String::as_str)
                .to_owned();
        };
        let shown: BTreeSet<&String> = folders.iter().map(|f| &self.names.shown[f]).collect();
        match shown.into_iter().collect::<Vec<_>>().as_slice() {
            [one] => (*one).clone(),
            _ => project.to_owned(),
        }
    }

    /// Whether `list` names the project of a block: by its name, by the name the page shows for
    /// it, or by every folder that has the name.
    fn named(&self, list: &[String], project: &str) -> bool {
        list.iter()
            .any(|n| n == project || *n == self.shown(project))
            || self
                .folders
                .get(project)
                .is_some_and(|folders| folders.iter().all(|f| self.names.named(list, f)))
    }
}

/// The pieces of `blocks` in `days`, after the hidden projects and the filter of `spec` have
/// left blocks out. `days` is sorted and checked.
fn select<'a>(
    sessions: &[SessionMinutes],
    blocks: &'a [Block],
    days: &[Day],
    spec: &Spec,
) -> Selected<'a> {
    let projects = Projects::new(sessions, spec);
    let mut pieces = Vec::new();
    let mut hidden = BTreeSet::new();
    for (index, block) in blocks.iter().enumerate() {
        let Some(from) = epoch(&block.start) else {
            continue;
        };
        let to = from + i64::try_from(block.seconds).unwrap_or(0);
        // The days that end after the block starts, in order, up to the one it ends in.
        let first = days.partition_point(|d| d.end * 60 <= from);
        let overlaps: Vec<(usize, i64, i64)> = days
            .iter()
            .enumerate()
            .skip(first)
            .take_while(|(_, d)| d.start * 60 < to)
            .map(|(day, d)| (day, from.max(d.start * 60), to.min(d.end * 60)))
            .filter(|(_, from, to)| from < to)
            .collect();
        if overlaps.is_empty() {
            continue;
        }
        let named = !block.project.is_empty();
        if named && projects.named(&spec.hidden_projects, &block.project) {
            hidden.insert(block.project.clone());
            continue;
        }
        let ticket = if block.ticket.is_empty() {
            NO_TICKET
        } else {
            &block.ticket
        };
        let keeps = |wanted: &Option<Vec<String>>, value: &str| {
            wanted.as_ref().is_none_or(|w| w.iter().any(|v| v == value))
        };
        if !keeps(&spec.filter.tickets, ticket)
            || spec
                .filter
                .projects
                .as_deref()
                .is_some_and(|list| !(named && projects.named(list, &block.project)))
        {
            continue;
        }
        let project = if named {
            projects.shown(&block.project)
        } else {
            String::new()
        };
        for (day, from, to) in overlaps {
            pieces.push(Piece {
                block: index,
                source: block,
                project: project.clone(),
                day,
                from,
                to,
            });
        }
    }
    Selected {
        pieces,
        hidden_projects: hidden.len(),
    }
}

fn key_of(piece: &Piece<'_>, grouping: ScreenGrouping, days: &[Day]) -> String {
    let or = |text: &str, none: &str| if text.is_empty() { none } else { text }.to_owned();
    match grouping {
        ScreenGrouping::Project => or(&piece.project, NO_PROJECT),
        ScreenGrouping::Ticket => or(&piece.source.ticket, NO_TICKET),
        ScreenGrouping::App => or(&piece.source.app, NO_APP),
        ScreenGrouping::Site => or(&piece.source.site, NO_SITE),
        ScreenGrouping::Day => days[piece.day].label.clone(),
    }
}

#[allow(clippy::cast_precision_loss)] // a total never nears 2^52 seconds
fn minutes(seconds: u64) -> f64 {
    seconds as f64 / 60.0
}

fn sorted(days: &[Day]) -> Result<Vec<Day>, DaysError> {
    check_days(days)?;
    let mut days = days.to_vec();
    days.sort_by_key(|d| d.start);
    Ok(days)
}

/// The time on screen in `days`, as the page shows it: after the hidden projects and the filter
/// of `spec` have left blocks out. `sessions` only name the projects; their minutes are not
/// read.
///
/// # Errors
/// Fails when a day is empty or two days overlap.
pub fn numbers(
    sessions: &[SessionMinutes],
    blocks: &[Block],
    days: &[Day],
    spec: &Spec,
) -> Result<Answer, DaysError> {
    let days = sorted(days)?;
    let Selected {
        pieces,
        hidden_projects,
    } = select(sessions, blocks, &days, spec);
    let total: u64 = pieces.iter().map(Piece::seconds).sum();
    let mut per_day = vec![0_u64; days.len()];
    for piece in &pieces {
        per_day[piece.day] += piece.seconds();
    }
    // key -> (seconds, the blocks that have time in it)
    let mut by_key: BTreeMap<String, (u64, BTreeSet<usize>)> = BTreeMap::new();
    if let Some(grouping) = spec.screen_grouping {
        if grouping == ScreenGrouping::Day {
            for day in &days {
                by_key.insert(day.label.clone(), (0, BTreeSet::new()));
            }
        }
        for piece in &pieces {
            let entry = by_key.entry(key_of(piece, grouping, &days)).or_default();
            entry.0 += piece.seconds();
            entry.1.insert(piece.block);
        }
    }
    let mut groups: Vec<(String, u64, usize)> = by_key
        .into_iter()
        .map(|(key, (seconds, members))| (key, seconds, members.len()))
        .collect();
    if spec.screen_grouping == Some(ScreenGrouping::Day) {
        // The days in order, not by their labels.
        let position: BTreeMap<&str, usize> = days
            .iter()
            .enumerate()
            .map(|(i, d)| (d.label.as_str(), i))
            .collect();
        groups.sort_by_key(|(key, ..)| position[key.as_str()]);
    } else {
        groups.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    }
    #[allow(clippy::cast_precision_loss)] // a total never nears 2^52 seconds
    let share = |seconds: u64| {
        if total == 0 {
            0.0
        } else {
            seconds as f64 / total as f64
        }
    };
    Ok(Answer {
        groups: groups
            .into_iter()
            .map(|(key, seconds, sessions)| Row {
                key,
                minutes: minutes(seconds),
                share: share(seconds),
                sessions,
            })
            .collect(),
        total_minutes: minutes(total),
        sessions: pieces
            .iter()
            .map(|p| p.block)
            .collect::<BTreeSet<_>>()
            .len(),
        days: days
            .iter()
            .zip(per_day)
            .map(|(day, seconds)| (day.label.clone(), minutes(seconds)))
            .collect(),
        hidden_projects,
    })
}

/// One block behind a group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockRow {
    /// Epoch seconds the block first counted in the group, and the end of the last second it
    /// counted in.
    pub start: i64,
    pub end: i64,
    /// The seconds it counted in the group: the sum over the rows is the group's number.
    pub seconds: u64,
    pub app: String,
    pub site: String,
    /// The project as the page names it.
    pub project: String,
    pub ticket: String,
}

/// The blocks that have time in `group`, the earliest first, with the seconds each has in it.
///
/// # Errors
/// Fails when the request has nothing to group by, or the days are not usable.
pub fn blocks_of(
    sessions: &[SessionMinutes],
    blocks: &[Block],
    days: &[Day],
    spec: &Spec,
    group: &str,
) -> Result<Vec<BlockRow>, String> {
    let grouping = spec
        .screen_grouping
        .ok_or("the request has nothing to group by, so it has no groups")?;
    let days = sorted(days).map_err(|e| e.to_string())?;
    let Selected { pieces, .. } = select(sessions, blocks, &days, spec);
    let mut rows: BTreeMap<usize, BlockRow> = BTreeMap::new();
    for piece in pieces
        .iter()
        .filter(|p| key_of(p, grouping, &days) == group)
    {
        let row = rows.entry(piece.block).or_insert_with(|| BlockRow {
            start: piece.from,
            end: piece.to,
            seconds: 0,
            app: piece.source.app.clone(),
            site: piece.source.site.clone(),
            project: piece.project.clone(),
            ticket: piece.source.ticket.clone(),
        });
        row.start = row.start.min(piece.from);
        row.end = row.end.max(piece.to);
        row.seconds += piece.seconds();
    }
    let mut rows: Vec<BlockRow> = rows.into_values().collect();
    rows.sort_by(|a, b| (a.start, &a.app).cmp(&(b.start, &b.app)));
    Ok(rows)
}
