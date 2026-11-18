//! The `render_dashboard` tool: the model looks at the dashboard as plain text.
//!
//! Without arguments it renders every widget of the current document with the numbers the page
//! shows. With a `period` and a `group_by` (and optionally `sessions` and a `filter`) it renders
//! that one table and changes nothing. When the file has errors, the tool gives them with their
//! places and says the page keeps the last good version, so a model that has edited the file sees
//! the result of its own edit.
//!
//! The numbers come from [`crate::dashboard_numbers::numbers`], the function the page's command
//! uses, and are rounded the way the page rounds them. The days of a period come from the window:
//! each `chat-send` may carry the calendar of the person's time zone (see [`RawWindow`]), and
//! without one the tool says it cannot tell, it never guesses a zone. The text is short and has a
//! fixed shape; it holds no text of a message.

use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::Value;
use time::{Date, Duration, Month};

use super::tool::ChatTool;
use crate::activity::rules::{Day, SessionMinutes, check_days};
use crate::dashboard::{Filter, GroupBy, Period, Sessions, WidgetKind};
use crate::dashboard_numbers::{
    Answer, MAX_DAYS, Payload, RawDay, Spec, numbers, payload_of, spec_of, spec_without_widget,
    whole_minute,
};
use crate::sync::lock;

/// The name the model calls the tool by.
pub const NAME: &str = "render_dashboard";

/// The most days a window may give: the previous month, this month and the rest of this week.
pub const MAX_WINDOW: usize = 100;

const BAD_ARGUMENTS: &str = "the arguments are not valid for this tool: give period and group_by \
together, or neither; period is one of today, this_week, last_week, this_month, last_month, \
last_7_days, last_30_days; sessions is one of own, unattended, all; group_by is one of project, \
ticket, tool, day; filter has projects, tickets and tools, each a list of names";

const NO_CALENDAR: &str = "the window has not told which days belong to which period yet, so the \
numbers cannot be counted; ask the person to try again from the window";

/// How the tool reads the sessions of the activity store.
pub type SessionSource = Arc<dyn Fn() -> Result<Vec<SessionMinutes>, &'static str> + Send + Sync>;

/// The calendar the window gave with its last message, shared by the chat and the tool.
#[derive(Clone, Default)]
pub struct Calendar(Arc<Mutex<Option<Window>>>);

struct Window {
    today: Date,
    days: Vec<Day>,
}

/// The calendar as a `chat-send` carries it: `today` and the consecutive `days` of the person's
/// time zone, oldest first, each with its label (`2026-11-18`) and its start and end in UTC on a
/// whole minute. It holds at least the days of every period the tool may ask for: from the first
/// day of the previous month to the end of the current month and of the current week.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawWindow {
    today: String,
    days: Vec<RawDay>,
}

fn date_of(label: &str) -> Option<Date> {
    let mut parts = label.splitn(3, '-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    if year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return None;
    }
    let month = Month::try_from(month.parse::<u8>().ok()?).ok()?;
    Date::from_calendar_date(year.parse().ok()?, month, day.parse().ok()?).ok()
}

fn label_of(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

impl Calendar {
    /// Keeps the calendar of the window.
    ///
    /// # Errors
    /// When the labels are not consecutive dates, a day is not on a whole minute in UTC, two days
    /// overlap, or `today` is not one of the days.
    pub fn set(&self, window: RawWindow) -> Result<(), &'static str> {
        const INVALID: &str = "invalid window";
        if window.days.is_empty() || window.days.len() > MAX_WINDOW {
            return Err(INVALID);
        }
        let today = date_of(&window.today).ok_or(INVALID)?;
        let mut days = Vec::with_capacity(window.days.len());
        let mut expected: Option<Date> = None;
        for raw in window.days {
            let date = date_of(&raw.label).ok_or(INVALID)?;
            if expected.is_some_and(|next| next != date) {
                return Err(INVALID);
            }
            expected = date.next_day();
            days.push(Day {
                label: raw.label,
                start: whole_minute(&raw.start).ok_or(INVALID)?,
                end: whole_minute(&raw.end).ok_or(INVALID)?,
            });
        }
        check_days(&days).map_err(|_| INVALID)?;
        if !days.iter().any(|d| date_of(&d.label) == Some(today)) {
            return Err(INVALID);
        }
        *lock(&self.0) = Some(Window { today, days });
        Ok(())
    }

    /// The days of `period`, in order.
    fn days_of(&self, period: Period) -> Result<Vec<Day>, String> {
        let guard = lock(&self.0);
        let window = guard.as_ref().ok_or_else(|| NO_CALENDAR.to_owned())?;
        let (first, last) = bounds(window.today, period);
        let mut days = Vec::new();
        let mut date = first;
        while date <= last {
            let label = label_of(date);
            let Some(day) = window.days.iter().find(|d| d.label == label) else {
                return Err(format!(
                    "the window has not given the days of {}",
                    period_name(period)
                ));
            };
            days.push(day.clone());
            date += Duration::days(1);
        }
        if days.len() > MAX_DAYS {
            return Err(format!("{} has too many days", period_name(period)));
        }
        Ok(days)
    }
}

/// The first and the last day of `period`. Weeks start on Monday, and a week or a month is the
/// whole of it, as the page counts it.
fn bounds(today: Date, period: Period) -> (Date, Date) {
    let monday = today - Duration::days(i64::from(today.weekday().number_days_from_monday()));
    let month = |date: Date| {
        let first = date.replace_day(1).unwrap_or(date);
        let next = if first.month() == Month::December {
            Date::from_calendar_date(first.year() + 1, Month::January, 1)
        } else {
            Date::from_calendar_date(first.year(), first.month().next(), 1)
        };
        (first, next.map_or(first, |n| n - Duration::days(1)))
    };
    match period {
        Period::Today => (today, today),
        Period::ThisWeek => (monday, monday + Duration::days(6)),
        Period::LastWeek => (monday - Duration::days(7), monday - Duration::days(1)),
        Period::ThisMonth => month(today),
        Period::LastMonth => month(month(today).0 - Duration::days(1)),
        Period::Last7Days => (today - Duration::days(6), today),
        Period::Last30Days => (today - Duration::days(29), today),
    }
}

const fn period_name(period: Period) -> &'static str {
    match period {
        Period::Today => "today",
        Period::ThisWeek => "this_week",
        Period::LastWeek => "last_week",
        Period::ThisMonth => "this_month",
        Period::LastMonth => "last_month",
        Period::Last7Days => "last_7_days",
        Period::Last30Days => "last_30_days",
    }
}

const fn group_name(group: GroupBy) -> &'static str {
    match group {
        GroupBy::Project => "project",
        GroupBy::Ticket => "ticket",
        GroupBy::Tool => "tool",
        GroupBy::Day => "day",
    }
}

const fn sessions_name(sessions: Sessions) -> &'static str {
    match sessions {
        Sessions::Own => "own sessions",
        Sessions::Unattended => "unattended sessions",
        Sessions::All => "all sessions",
    }
}

const fn kind_name(kind: WidgetKind) -> &'static str {
    match kind {
        WidgetKind::Total => "total",
        WidgetKind::BarsPerDay => "bars_per_day",
        WidgetKind::Table => "table",
        WidgetKind::DaySummary => "day_summary",
    }
}

/// `3 h 25 min`, `45 min`, `0 min`: whole minutes, as the page shows time.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a total is far below 2^52
fn minutes(value: f64) -> String {
    let whole = value.round().max(0.0) as u64;
    if whole >= 60 {
        format!("{} h {} min", whole / 60, whole % 60)
    } else {
        format!("{whole} min")
    }
}

fn sessions_count(count: usize) -> String {
    format!("{count} session{}", if count == 1 { "" } else { "s" })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    period: Option<Period>,
    sessions: Option<Sessions>,
    group_by: Option<GroupBy>,
    filter: Option<Filter>,
}

/// What a call asks for, read.
enum Ask {
    /// Every widget of the document.
    Document,
    /// One table for a period.
    Table {
        period: Period,
        sessions: Sessions,
        group_by: GroupBy,
        filter: Filter,
    },
}

fn read(arguments: &str) -> Result<Ask, String> {
    let text = if arguments.trim().is_empty() {
        "{}"
    } else {
        arguments
    };
    let raw: Arguments = serde_json::from_str(text).map_err(|_| BAD_ARGUMENTS.to_owned())?;
    match raw {
        Arguments {
            period: None,
            sessions: None,
            group_by: None,
            filter: None,
        } => Ok(Ask::Document),
        Arguments {
            period: Some(period),
            group_by: Some(group_by),
            sessions,
            filter,
        } => Ok(Ask::Table {
            period,
            sessions: sessions.unwrap_or(Sessions::Own),
            group_by,
            filter: filter.unwrap_or_default(),
        }),
        _ => Err(BAD_ARGUMENTS.to_owned()),
    }
}

/// The lines for an answer: the total, the groups and, for the days, one line each.
fn write_answer(text: &mut String, answer: &Answer, kind: Option<WidgetKind>, by: &str) {
    let _ = writeln!(
        text,
        "  Total: {} ({})",
        minutes(answer.total_minutes),
        sessions_count(answer.sessions)
    );
    if matches!(kind, Some(WidgetKind::BarsPerDay)) {
        for (label, value) in &answer.days {
            let _ = writeln!(text, "  {label}: {}", minutes(*value));
        }
    }
    if !answer.groups.is_empty() {
        let _ = writeln!(text, "  By {by}: time, share, sessions");
        for row in &answer.groups {
            let _ = writeln!(
                text,
                "  {}: {}, {:.0}%, {}",
                row.key,
                minutes(row.minutes),
                row.share * 100.0,
                row.sessions
            );
        }
    }
    if answer.hidden_projects > 0 {
        let _ = writeln!(
            text,
            "  Hidden projects with time: {}",
            answer.hidden_projects
        );
    }
}

fn errors_text(error: &Value) -> String {
    let place = match (error["kind"].as_str(), error["place"].as_str()) {
        (Some("file"), _) | (_, None) => "the file".to_owned(),
        (_, Some("")) => "the document".to_owned(),
        (_, Some(place)) => place.to_owned(),
    };
    format!(
        "The dashboard file has an error. The page keeps showing the last good version.\n\
         - {place}: {}",
        error["message"].as_str().unwrap_or("it cannot be used")
    )
}

struct RenderDashboard {
    dashboard: crate::dashboard_document::Dashboard,
    sessions: SessionSource,
    calendar: Calendar,
}

impl RenderDashboard {
    fn render(&self, ask: &Ask) -> Result<String, String> {
        let current = self.dashboard.to_json();
        if current.get("error").is_some() {
            return Ok(errors_text(&current["error"]));
        }
        let document = current["document"].clone();
        let payload = payload_of((!document.is_null()).then_some(&document))?;
        let mut text = String::new();
        match ask {
            Ask::Document => {
                let title = document["title"].as_str().unwrap_or("Dashboard");
                let _ = writeln!(text, "Dashboard {title:?}");
                self.widgets(&payload, &mut text)?;
            }
            Ask::Table {
                period,
                sessions,
                group_by,
                filter,
            } => {
                let spec =
                    spec_without_widget(&payload, *sessions, Some(*group_by), filter.clone());
                let _ = writeln!(
                    text,
                    "Table for {}, {}, by {}",
                    period_name(*period),
                    sessions_name(*sessions),
                    group_name(*group_by)
                );
                let answer = self.count(*period, &spec)?;
                write_answer(&mut text, &answer, None, group_name(*group_by));
            }
        }
        Ok(text.trim_end().to_owned())
    }

    fn widgets(&self, payload: &Payload, text: &mut String) -> Result<(), String> {
        for widget in &payload.widgets {
            let spec = spec_of(widget, payload);
            let period = widget.period.unwrap_or(payload.period);
            let by = spec.grouping.map(|_| match widget.kind {
                WidgetKind::DaySummary => GroupBy::Project,
                _ => widget.group_by.unwrap_or(GroupBy::Project),
            });
            let _ = writeln!(
                text,
                "Widget {} ({}, {}, {}{})",
                widget.id,
                kind_name(widget.kind),
                period_name(period),
                sessions_name(widget.sessions.unwrap_or(payload.sessions)),
                by.map_or(String::new(), |g| format!(", by {}", group_name(g)))
            );
            let answer = self.count(period, &spec)?;
            write_answer(text, &answer, Some(widget.kind), by.map_or("", group_name));
        }
        Ok(())
    }

    fn count(&self, period: Period, spec: &Spec) -> Result<Answer, String> {
        let days = self.calendar.days_of(period)?;
        let sessions = (self.sessions)().map_err(str::to_owned)?;
        numbers(&sessions, &days, spec).map_err(|e| e.to_string())
    }
}

impl ChatTool for RenderDashboard {
    fn name(&self) -> &'static str {
        NAME
    }

    fn description(&self) -> &'static str {
        "Looks at the person's dashboard as text and changes nothing. With no arguments it gives \
every widget of the dashboard with its title, period and kind and the numbers the page shows: a \
total, a table of groups with time, share and sessions, the time of each day. With period and \
group_by (and optionally sessions and filter) it gives only that table, for a question the \
dashboard does not show. When the dashboard file has errors it gives them with their places and \
says that the page keeps the last good version: call it after every edit of the file."
    }

    fn schema(&self) -> &'static str {
        r#"{"type":"object","properties":{"period":{"type":"string","enum":["today","this_week","last_week","this_month","last_month","last_7_days","last_30_days"],"description":"The period of the table. Give it with group_by."},"sessions":{"type":"string","enum":["own","unattended","all"],"description":"Whose sessions to count; own when left out."},"group_by":{"type":"string","enum":["project","ticket","tool","day"],"description":"What the table groups by. Give it with period."},"filter":{"type":"object","properties":{"projects":{"type":"array","items":{"type":"string"}},"tickets":{"type":"array","items":{"type":"string"}},"tools":{"type":"array","items":{"type":"string"}}},"additionalProperties":false,"description":"Keeps only what it lists; a list left out means no limit."}},"additionalProperties":false}"#
    }

    fn line(&self, arguments: &str) -> String {
        if self.dashboard.to_json().get("error").is_some() {
            return "Checked your dashboard file".to_owned();
        }
        match read(arguments) {
            Ok(Ask::Table {
                period, group_by, ..
            }) => format!(
                "Looked at your dashboard: {}, by {}",
                period_name(period),
                group_name(group_by)
            ),
            _ => "Looked at your dashboard".to_owned(),
        }
    }

    fn run(&self, arguments: &str) -> Result<String, String> {
        self.render(&read(arguments)?)
    }
}

/// The tool, to be put once in the list the chat is given, after the file tools and before the
/// sub-agent tool.
#[must_use]
pub fn tool(
    dashboard: crate::dashboard_document::Dashboard,
    sessions: SessionSource,
    calendar: Calendar,
) -> Arc<dyn ChatTool> {
    Arc::new(RenderDashboard {
        dashboard,
        sessions,
        calendar,
    })
}

#[cfg(test)]
mod tests {
    use time::Weekday;

    use super::*;

    fn date(label: &str) -> Date {
        date_of(label).unwrap()
    }

    #[test]
    fn the_periods_are_whole_calendar_ranges_that_start_on_monday() {
        // 2026-11-18 is a Wednesday.
        let today = date("2026-11-18");
        assert_eq!(today.weekday(), Weekday::Wednesday);
        let of = |p| {
            let (a, b) = bounds(today, p);
            (label_of(a), label_of(b))
        };
        assert_eq!(
            of(Period::Today),
            ("2026-11-18".into(), "2026-11-18".into())
        );
        assert_eq!(
            of(Period::ThisWeek),
            ("2026-11-16".into(), "2026-11-22".into())
        );
        assert_eq!(
            of(Period::LastWeek),
            ("2026-11-09".into(), "2026-11-15".into())
        );
        assert_eq!(
            of(Period::ThisMonth),
            ("2026-11-01".into(), "2026-11-30".into())
        );
        assert_eq!(
            of(Period::LastMonth),
            ("2026-10-01".into(), "2026-10-31".into())
        );
        assert_eq!(
            of(Period::Last7Days),
            ("2026-11-12".into(), "2026-11-18".into())
        );
        assert_eq!(
            of(Period::Last30Days),
            ("2026-10-20".into(), "2026-11-18".into())
        );
    }

    #[test]
    fn a_month_before_january_and_a_leap_february_are_whole() {
        let (a, b) = bounds(date("2028-01-10"), Period::LastMonth);
        assert_eq!(
            (label_of(a), label_of(b)),
            ("2027-12-01".into(), "2027-12-31".into())
        );
        let (a, b) = bounds(date("2028-03-02"), Period::LastMonth);
        assert_eq!(
            (label_of(a), label_of(b)),
            ("2028-02-01".into(), "2028-02-29".into())
        );
    }

    #[test]
    fn minutes_are_rounded_to_whole_minutes_as_the_page_does() {
        assert_eq!(minutes(0.0), "0 min");
        assert_eq!(minutes(44.5), "45 min");
        assert_eq!(minutes(59.4), "59 min");
        assert_eq!(minutes(205.0), "3 h 25 min");
    }

    #[test]
    fn arguments_are_all_or_a_period_with_a_grouping() {
        assert!(matches!(read(""), Ok(Ask::Document)));
        assert!(matches!(read("{}"), Ok(Ask::Document)));
        assert!(matches!(
            read(r#"{"period":"this_week","group_by":"tool"}"#),
            Ok(Ask::Table { .. })
        ));
        for bad in [
            r#"{"period":"this_week"}"#,
            r#"{"group_by":"tool"}"#,
            r#"{"sessions":"all"}"#,
            r#"{"period":"next_week","group_by":"tool"}"#,
            r#"{"period":"today","group_by":"tool","more":1}"#,
            "[1]",
        ] {
            assert_eq!(read(bad).err().as_deref(), Some(BAD_ARGUMENTS), "{bad}");
        }
    }
}
