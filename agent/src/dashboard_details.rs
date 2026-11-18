//! What the dashboard shows beside its numbers: the sessions behind a group, the summary of a
//! day, and how far the data is.
//!
//! Each answer is a plain function of its input, built on [`crate::dashboard_numbers::select`] and
//! the time rules of [`crate::activity::rules`], so a session listed here is one the numbers
//! counted. No answer holds the text of a message.
//!
//! * `dashboard-sessions`: the request of `dashboard-numbers` (a `widget`, or `sessions`, `filter`
//!   and `group_by`) with the `group` to open, and `offset`, `limit` and `utc_offset_minutes` (the
//!   user's offset from UTC, `0` when left out; the times of the answer are in it). The answer
//!   lists the sessions that have time in the group, the earliest first, a page at a time. A
//!   session shows when it first and last counted in the group, how many minutes it counted in,
//!   and the first characters of its id.
//! * `dashboard-summary`: the request of `dashboard-numbers` with one `day` in place of `days`,
//!   and no `group_by`. The answer is one text line for each ticket and, for time with no ticket,
//!   one line for each project.
//! * `dashboard-state`: no argument. `reading` while the store is built, with the records read,
//!   `empty` when it holds nothing, `ready` otherwise, or `unavailable`; `tools_off` names the tools
//!   whose collection is switched off.

use serde_json::{Map, Value, json};
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};
use tokio::runtime::Handle;

use crate::activity::rules::{
    Day, Grouping, Kind, PARTS_PER_MINUTE, SessionMinutes, Span, calculate_spans, check_days,
};
use crate::activity::{Feed, Readiness};
use crate::dashboard_document::Dashboard;
use crate::dashboard_numbers::{Request, read_request, select};
use crate::ipc::error_response;
use crate::settings::Settings;

/// Sessions on a page when the request does not say.
pub const DEFAULT_PAGE: usize = 50;
/// The most sessions on a page.
pub const MAX_PAGE: usize = 200;
/// Characters of a session id the answer shows.
const SHORT_ID: usize = 8;
/// The offsets from UTC that a clock uses, in minutes.
const OFFSETS: std::ops::RangeInclusive<i32> = -720..=840;

const NOT_AN_OBJECT: &str = "the request is not a JSON object the command knows";

/// One session of a group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRow {
    /// When the session first counted in the group, in the user's time (RFC 3339).
    pub start: String,
    /// The end of the last minute it counted in.
    pub end: String,
    /// The minutes it counted in. A minute shared with other groups counts whole.
    pub minutes: u64,
    pub tool: String,
    /// The project as the page names it.
    pub project: String,
    pub ticket: String,
    /// The first characters of the id of the session.
    pub session: String,
}

/// One page of the sessions of a group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionsPage {
    /// The sessions of the group, all pages together; it is the `sessions` of the group in the
    /// numbers.
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub rows: Vec<SessionRow>,
}

impl SessionsPage {
    /// The JSON line the socket sends.
    #[must_use]
    pub fn to_json(&self, group: &str) -> Value {
        let end = self.offset + self.rows.len();
        json!({
            "group": group,
            "total": self.total,
            "offset": self.offset,
            "limit": self.limit,
            "next_offset": (end < self.total).then_some(end),
            "sessions": self.rows.iter().map(|r| json!({
                "start": r.start,
                "end": r.end,
                "minutes": r.minutes,
                "tool": r.tool,
                "project": r.project,
                "ticket": r.ticket,
                "session": r.session,
            })).collect::<Vec<_>>(),
        })
    }
}

/// A minute since the epoch as a time at `offset`.
fn clock(minute: i64, offset: UtcOffset) -> String {
    OffsetDateTime::from_unix_timestamp(minute.saturating_mul(60))
        .ok()
        .and_then(|time| time.to_offset(offset).format(&Rfc3339).ok())
        .unwrap_or_default()
}

fn offset_of(minutes: i32) -> Result<UtcOffset, String> {
    let bad = || "utc_offset_minutes is not an offset from UTC".to_owned();
    if !OFFSETS.contains(&minutes) {
        return Err(bad());
    }
    UtcOffset::from_whole_seconds(minutes * 60).map_err(|_| bad())
}

/// The sessions that have time in `group`, one page of them, as the numbers of `spec` counted.
///
/// # Errors
/// Fails when `spec` has no grouping, or the days are not usable.
pub fn sessions_of(
    sessions: &[SessionMinutes],
    days: &[Day],
    spec: &crate::dashboard_numbers::Spec,
    group: &str,
    utc_offset_minutes: i32,
    (offset, limit): (usize, usize),
) -> Result<SessionsPage, String> {
    let grouping = spec
        .grouping
        .ok_or("the request has nothing to group by, so it has no groups")?;
    let zone = offset_of(utc_offset_minutes)?;
    let selection = select(sessions, days, spec);
    let kind = spec.kind.unwrap_or(Kind::Person);
    let (_, spans) = calculate_spans(&selection.kept, days, kind, grouping, &selection.shown)
        .map_err(|e| e.to_string())?;
    let mut spans: Vec<Span> = spans.get(group).cloned().unwrap_or_default();
    let kept = &selection.kept;
    spans.sort_by(|a, b| {
        (a.first, &kept[a.session].tool, &kept[a.session].session).cmp(&(
            b.first,
            &kept[b.session].tool,
            &kept[b.session].session,
        ))
    });
    let total = spans.len();
    let rows = spans
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|span| {
            let session = &kept[span.session];
            SessionRow {
                start: clock(span.first, zone),
                end: clock(span.last + 1, zone),
                minutes: span.minutes,
                tool: session.tool.clone(),
                project: selection
                    .shown
                    .get(&session.project)
                    .unwrap_or(&session.project)
                    .clone(),
                ticket: if session.ticket.is_empty() {
                    crate::activity::assign::NO_TICKET.to_owned()
                } else {
                    session.ticket.clone()
                },
                session: session.session.chars().take(SHORT_ID).collect(),
            }
        })
        .collect();
    Ok(SessionsPage {
        total,
        offset,
        limit,
        rows,
    })
}

/// A time of whole minutes as a person says it: `1 h 20 min`, `2 h`, `45 min`.
fn spoken(minutes: f64) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a day has 1440 minutes
    let whole = minutes.round() as u64;
    match (whole / 60, whole % 60) {
        (0, 0) => "under a minute".to_owned(),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// The summary of one day: the lines, and the minutes of the day.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub lines: Vec<String>,
    pub total_minutes: f64,
}

/// One line for each ticket with time in `day`, longest first, and one for each project with
/// time that has no ticket, as `No ticket (project)`.
///
/// # Errors
/// Fails when the day ends before it starts.
pub fn summary_of(
    sessions: &[SessionMinutes],
    day: &Day,
    spec: &crate::dashboard_numbers::Spec,
) -> Result<Summary, String> {
    let days = std::slice::from_ref(day);
    let selection = select(sessions, days, spec);
    let kind = spec.kind.unwrap_or(Kind::Person);
    let (report, _) = calculate_spans(
        &selection.kept,
        days,
        kind,
        Grouping::TicketOrProject,
        &selection.shown,
    )
    .map_err(|e| e.to_string())?;
    #[allow(clippy::cast_precision_loss)] // a day never nears 2^52 parts
    let minutes = |parts: u64| parts as f64 / PARTS_PER_MINUTE as f64;
    Ok(Summary {
        lines: report
            .groups
            .iter()
            .map(|g| format!("{}: {}", g.key, spoken(minutes(g.parts))))
            .collect(),
        total_minutes: minutes(report.total_parts),
    })
}

/// The state of the data, as the page shows it. `empty` is what the store holds when it can be
/// read, and is not known while it cannot.
#[must_use]
pub fn state_json(readiness: Readiness, empty: Option<bool>, tools_off: &[&str]) -> Value {
    let mut state = match (readiness, empty) {
        (Readiness::Reading { records_done }, _) => {
            json!({ "state": "reading", "records_done": records_done })
        }
        (Readiness::Ready, Some(true)) => json!({ "state": "empty" }),
        (Readiness::Ready, _) => json!({ "state": "ready" }),
        (Readiness::Unavailable, _) => json!({ "state": "unavailable" }),
    };
    state["tools_off"] = json!(tools_off);
    state
}

/// The request object, with the fields only this command reads taken out of it.
fn object_of(argument: &str) -> Result<Map<String, Value>, String> {
    match serde_json::from_str(argument) {
        Ok(Value::Object(object)) => Ok(object),
        _ => Err(NOT_AN_OBJECT.to_owned()),
    }
}

fn number(object: &mut Map<String, Value>, name: &str) -> Result<Option<i64>, String> {
    match object.remove(name) {
        None => Ok(None),
        Some(value) => value
            .as_i64()
            .map(Some)
            .ok_or_else(|| format!("{name} is not a whole number")),
    }
}

/// A request read for `dashboard-sessions`.
struct SessionsRequest {
    request: Request,
    group: String,
    utc_offset_minutes: i32,
    page: (usize, usize),
}

fn read_sessions_request(
    argument: &str,
    document: Option<&Value>,
) -> Result<SessionsRequest, String> {
    let mut object = object_of(argument)?;
    let Some(Value::String(group)) = object.remove("group") else {
        return Err("group is the name of the group to open".to_owned());
    };
    let offset = usize::try_from(number(&mut object, "offset")?.unwrap_or(0))
        .map_err(|_| "offset is not a position in the list".to_owned())?;
    let limit = match number(&mut object, "limit")? {
        None => DEFAULT_PAGE,
        Some(n) => usize::try_from(n)
            .ok()
            .filter(|n| (1..=MAX_PAGE).contains(n))
            .ok_or_else(|| format!("limit is from 1 to {MAX_PAGE}"))?,
    };
    let utc_offset_minutes = match number(&mut object, "utc_offset_minutes")? {
        None => 0,
        Some(n) => i32::try_from(n).map_err(|_| "utc_offset_minutes is too big".to_owned())?,
    };
    let request = read_request(&Value::Object(object).to_string(), document)?;
    Ok(SessionsRequest {
        request,
        group,
        utc_offset_minutes,
        page: (offset, limit),
    })
}

/// The JSON line for a `dashboard-sessions` request, given the dashboard `document` and a way to
/// read the sessions, which runs only for a request that is fine.
#[must_use]
pub fn respond_sessions(
    argument: &str,
    document: Option<&Value>,
    load: impl FnOnce() -> Result<Vec<SessionMinutes>, &'static str>,
) -> String {
    let read = match read_sessions_request(argument, document) {
        Ok(read) => read,
        Err(message) => return error_response(&message),
    };
    // The days are checked before the store is read: a bad set costs nothing.
    if let Err(e) = check_days(&read.request.days) {
        return error_response(&e.to_string());
    }
    let sessions = match load() {
        Ok(sessions) => sessions,
        Err(message) => return error_response(message),
    };
    match sessions_of(
        &sessions,
        &read.request.days,
        &read.request.spec,
        &read.group,
        read.utc_offset_minutes,
        read.page,
    ) {
        Ok(page) => page.to_json(&read.group).to_string(),
        Err(message) => error_response(&message),
    }
}

/// The JSON line for a `dashboard-summary` request.
#[must_use]
pub fn respond_summary(
    argument: &str,
    document: Option<&Value>,
    load: impl FnOnce() -> Result<Vec<SessionMinutes>, &'static str>,
) -> String {
    let read = || -> Result<Request, String> {
        let mut object = object_of(argument)?;
        if object.contains_key("days") || object.contains_key("group_by") {
            return Err("the summary is of one day, in its own grouping".to_owned());
        }
        let day = object.remove("day").ok_or("day is the day to summarise")?;
        object.insert("days".to_owned(), json!([day]));
        read_request(&Value::Object(object).to_string(), document)
    };
    let request = match read() {
        Ok(request) => request,
        Err(message) => return error_response(&message),
    };
    // The days are checked before the store is read: a bad set costs nothing.
    if let Err(e) = check_days(&request.days) {
        return error_response(&e.to_string());
    }
    let sessions = match load() {
        Ok(sessions) => sessions,
        Err(message) => return error_response(message),
    };
    match summary_of(&sessions, &request.days[0], &request.spec) {
        Ok(summary) => json!({
            "day": request.days[0].label,
            "total_minutes": summary.total_minutes,
            "lines": summary.lines,
        })
        .to_string(),
        Err(message) => error_response(&message),
    }
}

/// Answers `dashboard-sessions`, `dashboard-summary` and `dashboard-state` from the activity
/// store, the dashboard document and the settings.
#[derive(Clone)]
pub struct Details {
    feed: Feed,
    dashboard: Dashboard,
    settings: Option<Settings>,
    runtime: Handle,
}

impl Details {
    /// `runtime` runs the reads of the store; a command is answered on a thread of its own.
    /// Without `settings` no tool is reported as off.
    #[must_use]
    pub fn new(
        feed: Feed,
        dashboard: Dashboard,
        settings: Option<Settings>,
        runtime: Handle,
    ) -> Self {
        Self {
            feed,
            dashboard,
            settings,
            runtime,
        }
    }

    fn document(&self) -> Option<Value> {
        let document = self.dashboard.to_json()["document"].clone();
        (!document.is_null()).then_some(document)
    }

    fn load(&self) -> Result<Vec<SessionMinutes>, &'static str> {
        let store = self.feed.readable_store()?;
        self.runtime.block_on(store.session_minutes()).map_err(|e| {
            eprintln!("dashboard details: {e}");
            "the activity time could not be read"
        })
    }

    /// The JSON line for `dashboard-sessions`.
    #[must_use]
    pub fn sessions(&self, argument: &str) -> String {
        respond_sessions(argument, self.document().as_ref(), || self.load())
    }

    /// The JSON line for `dashboard-summary`.
    #[must_use]
    pub fn summary(&self, argument: &str) -> String {
        respond_summary(argument, self.document().as_ref(), || self.load())
    }

    /// The JSON line for `dashboard-state`.
    #[must_use]
    pub fn state(&self) -> String {
        let readiness = self.feed.readiness();
        let empty = if readiness == Readiness::Ready {
            let looked = self
                .feed
                .readable_store()
                .ok()
                .and_then(|store| self.runtime.block_on(store.is_empty()).ok());
            match looked {
                Some(empty) => Some(empty),
                None => return error_response("the activity time could not be read"),
            }
        } else {
            None
        };
        let off = self.settings.as_ref().map(Settings::tools_off);
        state_json(readiness, empty, off.as_deref().unwrap_or_default()).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_said_in_hours_and_minutes() {
        assert_eq!(spoken(0.2), "under a minute");
        assert_eq!(spoken(45.0), "45 min");
        assert_eq!(spoken(60.0), "1 h");
        assert_eq!(spoken(80.4), "1 h 20 min");
    }

    #[test]
    fn a_minute_is_shown_in_the_users_offset() {
        let zone = offset_of(120).unwrap();
        assert_eq!(clock(0, zone), "1970-01-01T02:00:00+02:00");
        assert_eq!(
            clock(0, offset_of(-90).unwrap()),
            "1969-12-31T22:30:00-01:30"
        );
        assert!(offset_of(5000).is_err());
    }
}
