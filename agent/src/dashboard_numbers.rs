//! The numbers of the dashboard: what a widget, or a period with a kind, a filter and a grouping,
//! is worth in minutes.
//!
//! [`numbers`] is a plain function of its input and is the one place that applies the settings of
//! the page to the time rules of [`crate::activity::rules`]: it never counts a minute itself. The
//! socket command `dashboard-numbers` ([`Numbers`]) calls it through [`respond`]; a chat tool that
//! answers the same question calls `numbers` too, so the two cannot differ.
//!
//! The answer holds groups with minutes, shares and session counts (`session_count`), the total,
//! the minutes of each day, and `hidden_project_count`: the hidden folders that have time in the
//! days, of any kind of session. It holds no text of a message.
//!
//! * Sessions of a hidden project, and sessions the filter does not keep, are left out before the
//!   rules run, as if they were not there: a minute they shared goes whole to the sessions that
//!   stay. A project is named by its folder, by the name a report gives it, or by the name the
//!   page shows for it. A project keeps the name it has among all the sessions, so hiding one of
//!   two folders called `api` does not rename the other one: the page's names do not move with
//!   what is hidden. (The time rules alone name the projects they are given.)
//! * Projects the page shows under one name are one group, and share a minute as one.
//! * The caller sends the start and the end of each day in UTC, as the page counts days in its
//!   own time zone. A day starts and ends on a whole minute.
//!
//! A request is a JSON object: `days` (needed), and either `widget` (the id of a widget of the
//! document; its own sessions, filter and grouping count) or `sessions` (`own`, `unattended`,
//! `all`; `own` when left out), `filter` and `group_by`. The settings of the page (hidden projects
//! and names) come from the document, so a request is refused while there is none that is usable.
//! The only things of a request an error repeats are a short widget id and the label of a day.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::runtime::Handle;

use crate::activity::Feed;
use crate::activity::assign::{self, NO_TICKET};
use crate::activity::rules::{
    Day, DaysError, Group, Grouping, Kind, PARTS_PER_MINUTE, REACH, SessionMinutes,
    calculate_named, check_days, minute_number,
};
use crate::dashboard::{ActivityDashboardV1, DashboardV1, Filter, GroupBy, Sessions, WidgetKind};
use crate::dashboard_document::Dashboard;

/// The most days one request may ask for: a month and a half is more than any period of a page.
pub const MAX_DAYS: usize = 62;
/// The longest a widget id in a request may be before it is not repeated in an error.
const MAX_ECHO: usize = 64;

/// What to count, apart from the days.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Spec {
    pub kind: Option<Kind>,
    /// What to split by; none gives the total and the days only.
    pub grouping: Option<Grouping>,
    pub filter: Filter,
    /// The projects the page leaves out.
    pub hidden_projects: Vec<String>,
    /// The name the page shows for a project, by the project's folder or by the name a report
    /// gives it.
    pub project_names: BTreeMap<String, String>,
}

/// One group of the answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub key: String,
    pub minutes: f64,
    pub share: f64,
    pub sessions: usize,
}

/// The answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub groups: Vec<Row>,
    pub total_minutes: f64,
    pub sessions: usize,
    /// The minutes of each of the days, in the order of the days.
    pub days: Vec<(String, f64)>,
    /// The hidden projects that have time in the days.
    pub hidden_projects: usize,
}

#[allow(clippy::cast_precision_loss)] // a total never nears 2^52 parts
fn minutes(parts: u64) -> f64 {
    parts as f64 / PARTS_PER_MINUTE as f64
}

fn row(group: Group) -> Row {
    Row {
        minutes: minutes(group.parts),
        key: group.key,
        share: group.share,
        sessions: group.sessions,
    }
}

impl Answer {
    /// The JSON line the socket sends.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "total_minutes": self.total_minutes,
            "session_count": self.sessions,
            "hidden_project_count": self.hidden_projects,
            "groups": self.groups.iter().map(|g| json!({
                "key": g.key,
                "minutes": g.minutes,
                "share": g.share,
                "session_count": g.sessions,
            })).collect::<Vec<_>>(),
            "days": self.days.iter().map(|(label, minutes)| json!({
                "label": label,
                "minutes": minutes,
            })).collect::<Vec<_>>(),
        })
    }
}

/// The time of `sessions` in `days` as the page shows it: by the time rules, after the hidden
/// projects and the filter of `spec` have left sessions out.
///
/// # Errors
/// Fails when a day is empty or two days overlap.
pub fn numbers(
    sessions: &[SessionMinutes],
    days: &[Day],
    spec: &Spec,
) -> Result<Answer, DaysError> {
    // Projects are named over all the sessions, so that a name does not depend on what is left.
    let labels = assign::project_labels(sessions.iter().map(|s| s.project.as_str()));
    let shown: BTreeMap<String, String> = labels
        .iter()
        .map(|(project, label)| {
            let name = spec
                .project_names
                .get(project)
                .or_else(|| spec.project_names.get(label))
                .unwrap_or(label);
            (project.clone(), name.clone())
        })
        .collect();
    let named = |list: &[String], project: &str| {
        list.iter()
            .any(|n| n == project || *n == labels[project] || *n == shown[project])
    };
    let (low, high) = (
        days.iter().map(|d| d.start).min().unwrap_or(0) - REACH,
        days.iter().map(|d| d.end).max().unwrap_or(0) + REACH,
    );
    // Only the minutes near the days can change the answer; a long history is not walked.
    let near_days = |session: &SessionMinutes| SessionMinutes {
        minutes: session
            .minutes
            .iter()
            .copied()
            .filter(|m| (low..high).contains(m))
            .collect(),
        ..session.clone()
    };
    let mut hidden_sessions: Vec<SessionMinutes> = Vec::new();
    let mut kept: Vec<SessionMinutes> = Vec::new();
    for session in sessions {
        if named(&spec.hidden_projects, &session.project) {
            hidden_sessions.push(near_days(session));
            continue;
        }
        let ticket = if session.ticket.is_empty() {
            NO_TICKET
        } else {
            &session.ticket
        };
        let keeps = |wanted: &Option<Vec<String>>, value: &str| {
            wanted.as_ref().is_none_or(|w| w.iter().any(|v| v == value))
        };
        if !keeps(&spec.filter.tools, &session.tool)
            || !keeps(&spec.filter.tickets, ticket)
            || spec
                .filter
                .projects
                .as_deref()
                .is_some_and(|list| !named(list, &session.project))
        {
            continue;
        }
        kept.push(near_days(session));
    }
    kept.retain(|s| !s.minutes.is_empty());

    // A hidden folder counts when the time rules give it time in the days, whatever the kind of
    // session: each folder is asked on its own, so the count does not depend on which other
    // folders are hidden.
    let folders: BTreeMap<String, String> = labels.keys().map(|p| (p.clone(), p.clone())).collect();
    let hidden = hidden_sessions
        .iter()
        .map(|s| s.project.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|project| {
            let own: Vec<SessionMinutes> = hidden_sessions
                .iter()
                .filter(|s| s.project == *project)
                .cloned()
                .collect();
            // The days were checked by the caller; a set that cannot be used counts none here.
            calculate_named(&own, days, Kind::Both, Grouping::Project, &folders)
                .is_ok_and(|report| report.total_parts > 0)
        })
        .count();
    let kind = spec.kind.unwrap_or(Kind::Person);
    let report = |grouping| calculate_named(&kept, days, kind, grouping, &shown);
    let per_day = report(Grouping::Day)?;
    let groups = match spec.grouping {
        None => Vec::new(),
        Some(Grouping::Day) => per_day.groups.clone(),
        Some(grouping) => report(grouping)?.groups,
    };
    Ok(Answer {
        groups: groups.into_iter().map(row).collect(),
        total_minutes: minutes(per_day.total_parts),
        sessions: per_day.sessions,
        days: per_day
            .groups
            .into_iter()
            .map(|g| (g.key, minutes(g.parts)))
            .collect(),
        hidden_projects: hidden,
    })
}

/// A request line, read.
struct Request {
    spec: Spec,
    days: Vec<Day>,
    /// What the widget draws, when the request names one.
    widget: Option<(String, WidgetKind)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDay {
    label: String,
    start: String,
    end: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRequest {
    days: Vec<RawDay>,
    widget: Option<String>,
    sessions: Option<Sessions>,
    filter: Option<Filter>,
    group_by: Option<GroupBy>,
}

fn kind_of(sessions: Sessions) -> Kind {
    match sessions {
        Sessions::Own => Kind::Person,
        Sessions::Unattended => Kind::Unattended,
        Sessions::All => Kind::Both,
    }
}

fn grouping_of(group_by: GroupBy) -> Grouping {
    match group_by {
        GroupBy::Project => Grouping::Project,
        GroupBy::Ticket => Grouping::Ticket,
        GroupBy::Tool => Grouping::Tool,
        GroupBy::Day => Grouping::Day,
    }
}

/// A UTC time that is a whole minute: `2026-11-11T00:00:00Z`, or `2026-11-11T00:00Z`.
fn whole_minute(time: &str) -> Option<i64> {
    let seconds = time.get(16..)?;
    if !matches!(seconds, "Z" | ":00Z") {
        return None;
    }
    minute_number(time)
}

fn read_days(raw: Vec<RawDay>) -> Result<Vec<Day>, String> {
    if raw.is_empty() {
        return Err("days is empty".to_owned());
    }
    if raw.len() > MAX_DAYS {
        return Err(format!("at most {MAX_DAYS} days can be asked for at once"));
    }
    raw.into_iter()
        .map(|day| {
            let bad = || {
                "a day starts and ends on a whole minute in UTC, like 2026-11-11T00:00:00Z"
                    .to_owned()
            };
            let (start, end) = (
                whole_minute(&day.start).ok_or_else(bad)?,
                whole_minute(&day.end).ok_or_else(bad)?,
            );
            Ok(Day {
                label: day.label,
                start,
                end,
            })
        })
        .collect()
}

/// The payload of the dashboard document, read once and typed. The document passed its type, so
/// a payload that does not read is a fault of the agent, and the request is refused.
type Payload = ActivityDashboardV1;

fn payload_of(document: Option<&Value>) -> Result<Payload, String> {
    let document = document.ok_or("the dashboard document is not available")?;
    serde_json::from_value::<DashboardV1<Payload>>(document.clone())
        .map(|typed| typed.payload)
        .map_err(|_| "the settings of the dashboard cannot be read".to_owned())
}

/// What a widget of the document counts: its own sessions or the page's, its filter and, for a
/// table, what it groups by; the summary of the day groups by project.
fn widget_spec(payload: Payload, id: &str) -> Result<(Spec, WidgetKind), String> {
    let Some(widget) = payload.widgets.into_iter().find(|w| w.id == id) else {
        let shown = if id.chars().count() <= MAX_ECHO {
            format!("there is no widget {id:?} in the dashboard")
        } else {
            "there is no such widget in the dashboard".to_owned()
        };
        return Err(shown);
    };
    let grouping = match widget.kind {
        WidgetKind::Table => widget.group_by.map(grouping_of),
        WidgetKind::DaySummary => Some(Grouping::Project),
        WidgetKind::Total | WidgetKind::BarsPerDay => None,
    };
    Ok((
        Spec {
            kind: Some(kind_of(widget.sessions.unwrap_or(payload.sessions))),
            grouping,
            filter: widget.filter.unwrap_or_default(),
            hidden_projects: payload.hidden_projects,
            project_names: payload.project_names,
        },
        widget.kind,
    ))
}

fn read_request(argument: &str, document: Option<&Value>) -> Result<Request, String> {
    let raw: RawRequest = serde_json::from_str(argument)
        .map_err(|_| "the request is not a JSON object the command knows".to_owned())?;
    let days = read_days(raw.days)?;
    let payload = payload_of(document)?;
    if let Some(id) = raw.widget {
        if raw.sessions.is_some() || raw.filter.is_some() || raw.group_by.is_some() {
            return Err("a request names a widget or the counting, not both".to_owned());
        }
        let (spec, kind) = widget_spec(payload, &id)?;
        return Ok(Request {
            spec,
            days,
            widget: Some((id, kind)),
        });
    }
    let sessions = raw.sessions.unwrap_or(Sessions::Own);
    // The hidden projects and the names are the page's, whatever is counted.
    let spec = Spec {
        kind: Some(kind_of(sessions)),
        grouping: raw.group_by.map(grouping_of),
        filter: raw.filter.unwrap_or_default(),
        hidden_projects: payload.hidden_projects,
        project_names: payload.project_names,
    };
    Ok(Request {
        spec,
        days,
        widget: None,
    })
}

/// Answers `dashboard-numbers` from the activity store and the dashboard document.
#[derive(Clone)]
pub struct Numbers {
    feed: Feed,
    dashboard: Dashboard,
    runtime: Handle,
}

impl Numbers {
    /// `runtime` runs the read of the store; the command is answered on a thread of its own.
    #[must_use]
    pub fn new(feed: Feed, dashboard: Dashboard, runtime: Handle) -> Self {
        Self {
            feed,
            dashboard,
            runtime,
        }
    }

    /// The JSON line to send for a request.
    #[must_use]
    pub fn answer(&self, argument: &str) -> String {
        let document = self.dashboard.to_json()["document"].clone();
        respond(argument, (!document.is_null()).then_some(&document), || {
            let store = self.feed.readable_store()?;
            self.runtime.block_on(store.session_minutes()).map_err(|e| {
                // The reason stays in the agent's log, as the activity store's own failures do.
                eprintln!("dashboard numbers: {e}");
                "the activity time could not be read"
            })
        })
    }
}

/// The JSON line for a request `argument`, given the dashboard `document` (none when there is
/// no good one) and a way to read the sessions, which runs only for a request that is fine.
/// [`Numbers::answer`] is this with the live store.
#[must_use]
pub fn respond(
    argument: &str,
    document: Option<&Value>,
    sessions: impl FnOnce() -> Result<Vec<SessionMinutes>, &'static str>,
) -> String {
    let request = match read_request(argument, document) {
        Ok(request) => request,
        Err(message) => return crate::ipc::error_response(&message),
    };
    // The days are checked before the store is read: a bad set costs nothing.
    if let Err(e) = check_days(&request.days) {
        return crate::ipc::error_response(&e.to_string());
    }
    let sessions = match sessions() {
        Ok(sessions) => sessions,
        Err(message) => return crate::ipc::error_response(message),
    };
    match numbers(&sessions, &request.days, &request.spec) {
        Ok(answer) => {
            let mut reply = answer.to_json();
            if let Some((id, kind)) = request.widget {
                reply["widget"] = json!({ "id": id, "kind": kind });
            }
            reply.to_string()
        }
        Err(e) => crate::ipc::error_response(&e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(project: &str, minutes: &[i64]) -> SessionMinutes {
        SessionMinutes {
            tool: "claude".to_owned(),
            session: project.to_owned(),
            project: project.to_owned(),
            ticket: String::new(),
            unattended: false,
            minutes: minutes.to_vec(),
        }
    }

    fn day(label: &str, start: i64, end: i64) -> Day {
        Day {
            label: label.to_owned(),
            start,
            end,
        }
    }

    #[test]
    fn a_minute_is_whole_only_in_utc() {
        assert_eq!(whole_minute("1970-01-01T00:01:00Z"), Some(1));
        assert_eq!(whole_minute("1970-01-01T00:01Z"), Some(1));
        assert_eq!(whole_minute("1970-01-01T00:01:30Z"), None);
        assert_eq!(whole_minute("1970-01-01T00:01:00+02:00"), None);
        assert_eq!(whole_minute("1970-01-01T00:01:00"), None);
    }

    #[test]
    fn the_minutes_far_from_the_days_change_nothing() {
        let sessions = vec![session("/w/a", &[1, 2, 500, 501, 502, 1000, 1001])];
        let days = [day("d", 500, 600)];
        let spec = Spec {
            grouping: Some(Grouping::Project),
            ..Spec::default()
        };
        let clipped = numbers(&sessions, &days, &spec).unwrap();
        let names = assign::project_labels(["/w/a"]);
        let whole =
            calculate_named(&sessions, &days, Kind::Person, Grouping::Project, &names).unwrap();
        assert!((clipped.total_minutes - minutes(whole.total_parts)).abs() < f64::EPSILON);
        assert_eq!(clipped.groups.len(), whole.groups.len());
    }

    #[test]
    fn a_block_that_starts_before_the_days_counts_from_their_start() {
        // Active at 98 and 103: the pause of 4 minutes joins them, and minute 100 and 101 are
        // inside the day that starts at 100.
        let sessions = vec![session("/w/a", &[98, 103])];
        let answer = numbers(&sessions, &[day("d", 100, 200)], &Spec::default()).unwrap();
        assert!((answer.total_minutes - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_widget_that_is_not_in_the_document_is_refused_in_plain_words() {
        let document = crate::dashboard::default_activity();
        let reply = read_request(
            r#"{"widget":"nope","days":[{"label":"d","start":"2026-11-11T00:00:00Z","end":"2026-11-12T00:00:00Z"}]}"#,
            Some(&document),
        )
        .err()
        .unwrap();
        assert_eq!(reply, "there is no widget \"nope\" in the dashboard");
    }
}
