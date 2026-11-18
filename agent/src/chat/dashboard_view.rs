//! The `render_dashboard` tool: the model looks at the dashboard as plain text.
//!
//! Without arguments it renders every widget of the current document with the numbers the page
//! shows. With a `period` and a `group_by` (and optionally `sessions` and a `filter`) it renders
//! that one table and changes nothing. When the file has errors, the tool gives them with their
//! places and says the page keeps the last good version, so a model that has edited the file sees
//! the result of its own edit.
//!
//! The numbers come from [`crate::dashboard_numbers::numbers`], the function the page's command
//! uses, and are rounded the way the page rounds them. The days of a period are counted in the
//! time zone of this computer, from its clock, at every call. The
//! text is short and has a fixed shape; it holds no text of a message.

use std::fmt::Write as _;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use time::{Date, Duration, Month};

use super::tool::ChatTool;
use crate::activity::rules::{Day, SessionMinutes};
use crate::dashboard::{Filter, GroupBy, Period, Sessions, WidgetKind};
use crate::dashboard_numbers::{
    Answer, Payload, Spec, numbers, payload_of, spec_of, spec_without_widget, whole_minute,
};

/// The name the model calls the tool by.
pub const NAME: &str = "render_dashboard";

const BAD_ARGUMENTS: &str = "the arguments are not valid for this tool: give period and group_by \
together, or neither; period is one of today, this_week, last_week, this_month, last_month, \
last_7_days, last_30_days; sessions is one of own, unattended, all; group_by is one of project, \
ticket, tool, day; filter has projects, tickets and tools, each a list of names";

const NO_CALENDAR: &str = "the days of the periods cannot be worked out from the clock and the \
time zone of this computer, so the numbers cannot be counted";

/// How the tool reads the sessions of the activity store.
pub type SessionSource = Arc<dyn Fn() -> Result<Vec<SessionMinutes>, &'static str> + Send + Sync>;

/// The days of the periods, in the time zone of this computer.
#[derive(Clone)]
pub struct Calendar(Arc<dyn Fn() -> Option<Window> + Send + Sync>);

impl Default for Calendar {
    /// Reads the clock and the time zone of this computer at every call, so a day that has passed
    /// is seen at once. When the zone cannot be read the system falls back to UTC.
    fn default() -> Self {
        Self(Arc::new(|| window_in(&chrono::Local::now())))
    }
}

/// The days the periods are made of, and which of them is today.
#[derive(Clone)]
pub struct Window {
    today: Date,
    days: Vec<Day>,
}

impl Calendar {
    /// A calendar that stays at `now`, in the zone of `now`: the clock of a test.
    #[must_use]
    pub fn at<Tz>(now: &chrono::DateTime<Tz>) -> Self
    where
        Tz: chrono::TimeZone + Send + Sync + 'static,
        Tz::Offset: Send + Sync,
    {
        let window = window_in(now);
        Self(Arc::new(move || window.clone()))
    }

    /// The days of `period`, in order.
    fn days_of(&self, period: Period) -> Result<Vec<Day>, String> {
        let window = (self.0)().ok_or_else(|| NO_CALENDAR.to_owned())?;
        let (first, last) = bounds(window.today, period);
        let mut days = Vec::new();
        let mut date = first;
        while date <= last {
            let label = date.to_string();
            let Some(day) = window.days.iter().find(|d| d.label == label) else {
                return Err(NO_CALENDAR.to_owned());
            };
            days.push(day.clone());
            date += Duration::days(1);
        }
        Ok(days)
    }
}

/// The first and the last day of `period`. Weeks start on Monday, and a week or a month is the
/// whole of it, as the page counts it.
fn bounds(today: Date, period: Period) -> (Date, Date) {
    let monday = today - Duration::days(i64::from(today.weekday().number_days_from_monday()));
    let month = |date: Date| {
        let first = date - Duration::days(i64::from(date.day()) - 1);
        let length = time::util::days_in_month(first.month(), first.year());
        (first, first + Duration::days(i64::from(length) - 1))
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

/// The days from the earlier of the first of the previous month and 29 days before today, to the
/// end of the current month and of the current week: every period the tool may ask for. A day
/// runs from the first instant of its date in the zone of `now` to the first instant of the next
/// date, so it is 23 or 25 hours when the clocks change, and when the clocks jump over a midnight
/// the day starts at the first instant after the jump. `None` when a day does not start on a whole
/// minute of UTC.
fn window_in<Tz: chrono::TimeZone>(now: &chrono::DateTime<Tz>) -> Option<Window> {
    use chrono::Datelike;
    let today = Date::from_calendar_date(
        now.year(),
        Month::try_from(u8::try_from(now.month()).ok()?).ok()?,
        u8::try_from(now.day()).ok()?,
    )
    .ok()?;
    let first = bounds(today, Period::LastMonth)
        .0
        .min(bounds(today, Period::Last30Days).0);
    let last = bounds(today, Period::ThisMonth)
        .1
        .max(bounds(today, Period::ThisWeek).1);
    let zone = now.timezone();
    let start_of = |date: Date| -> Option<i64> {
        let midnight = chrono::NaiveDate::from_ymd_opt(
            date.year(),
            u32::from(u8::from(date.month())),
            u32::from(date.day()),
        )?
        .and_hms_opt(0, 0, 0)?;
        // A midnight the clocks jump over has no instant: take the first local time after the gap
        // that has one (gaps are a multiple of a quarter of an hour, and shorter than a day).
        let start = (0..96)
            .find_map(|step| {
                zone.from_local_datetime(&(midnight + chrono::Duration::minutes(15 * step)))
                    .earliest()
            })?
            .to_utc()
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        whole_minute(&start)
    };
    let mut days = Vec::new();
    let mut date = first;
    while date <= last {
        let next = date.next_day()?;
        days.push(Day {
            label: date.to_string(),
            start: start_of(date)?,
            end: start_of(next)?,
        });
        date = next;
    }
    Some(Window { today, days })
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
    // The reader always gives a kind, a place (empty for the whole document) and a message.
    let place = match (
        error["kind"].as_str(),
        error["place"].as_str().unwrap_or(""),
    ) {
        (Some("file"), _) => "the file",
        (_, "") => "the document",
        (_, place) => place,
    };
    format!(
        "The dashboard file has an error. The page keeps showing the last good version.\n\
         - {place}: {}",
        error["message"].as_str().unwrap_or("")
    )
}

struct RenderDashboard {
    dashboard: crate::dashboard_document::Dashboard,
    sessions: SessionSource,
    calendar: Calendar,
}

impl RenderDashboard {
    fn render(&self, ask: &Ask) -> Result<String, String> {
        self.dashboard.settle();
        let current = self.dashboard.to_json();
        if current.get("error").is_some() {
            return Ok(errors_text(&current["error"]));
        }
        let document = current["document"].clone();
        let payload = payload_of((!document.is_null()).then_some(&document))?;
        let mut text = String::new();
        match ask {
            Ask::Document => {
                let title = document["title"].as_str().unwrap_or("");
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
            // Only a table names what it groups by (the document refuses one without), and a day
            // summary is counted by project.
            let by = spec.grouping.and(match widget.kind {
                WidgetKind::DaySummary => Some(GroupBy::Project),
                _ => widget.group_by,
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
every widget of the dashboard with its id, kind, period and sessions and the numbers the page shows: a \
total, a table of groups with time, share and sessions, the time of each day. With period and \
group_by (and optionally sessions and filter) it gives only that table, for a question the \
dashboard does not show. When the dashboard file has errors it gives them with their places and \
says that the page keeps the last good version: call it after every edit of the file."
    }

    fn schema(&self) -> &'static str {
        r#"{"type":"object","properties":{"period":{"type":"string","enum":["today","this_week","last_week","this_month","last_month","last_7_days","last_30_days"],"description":"The period of the table. Give it with group_by."},"sessions":{"type":"string","enum":["own","unattended","all"],"description":"Whose sessions to count; own when left out."},"group_by":{"type":"string","enum":["project","ticket","tool","day"],"description":"What the table groups by. Give it with period."},"filter":{"type":"object","properties":{"projects":{"type":"array","items":{"type":"string"}},"tickets":{"type":"array","items":{"type":"string"}},"tools":{"type":"array","items":{"type":"string"}}},"additionalProperties":false,"description":"Keeps only what it lists; a list left out means no limit."}},"additionalProperties":false}"#
    }

    fn line(&self, arguments: &str) -> String {
        self.dashboard.settle();
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
        let part = |range: std::ops::Range<usize>| label[range].parse::<i32>().unwrap();
        Date::from_calendar_date(
            part(0..4),
            Month::try_from(u8::try_from(part(5..7)).unwrap()).unwrap(),
            u8::try_from(part(8..10)).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn the_days_start_at_the_midnight_of_the_zone_not_of_utc() {
        use chrono::TimeZone;
        // 2026-11-18 01:00 in +05:30 is still the 17th in UTC.
        let zone = chrono::FixedOffset::east_opt(5 * 3600 + 1800).unwrap();
        let now = zone.with_ymd_and_hms(2026, 11, 18, 1, 0, 0).unwrap();
        let window = window_in(&now).unwrap();
        assert_eq!(window.today.to_string(), "2026-11-18");
        let today = window
            .days
            .iter()
            .find(|d| d.label == "2026-11-18")
            .unwrap();
        // Midnight +05:30 is 18:30Z the evening before.
        assert_eq!(whole_minute("2026-11-17T18:30:00Z"), Some(today.start));
        assert_eq!(whole_minute("2026-11-18T18:30:00Z"), Some(today.end));
        // From the first of last month (Oct 1) to the end of this month (Nov 30), whole.
        assert_eq!(window.days.first().unwrap().label, "2026-10-01");
        assert_eq!(window.days.last().unwrap().label, "2026-11-30");
        assert_eq!(window.days.len(), 61);
    }

    fn length_in_minutes(window: &Window, label: &str) -> i64 {
        let day = window.days.iter().find(|d| d.label == label).unwrap();
        day.end - day.start
    }

    #[test]
    fn a_day_is_23_or_25_hours_when_the_clocks_change() {
        use chrono::TimeZone;
        let zone = chrono_tz::Europe::London;
        let window = window_in(&zone.with_ymd_and_hms(2026, 3, 30, 12, 0, 0).unwrap()).unwrap();
        assert_eq!(length_in_minutes(&window, "2026-03-29"), 23 * 60);
        assert_eq!(length_in_minutes(&window, "2026-03-28"), 24 * 60);
        let window = window_in(&zone.with_ymd_and_hms(2026, 10, 26, 12, 0, 0).unwrap()).unwrap();
        assert_eq!(length_in_minutes(&window, "2026-10-25"), 25 * 60);
    }

    #[test]
    fn a_day_whose_midnight_does_not_exist_starts_after_the_jump() {
        use chrono::TimeZone;
        // Santiago puts the clocks from 00:00 to 01:00 on 2026-09-06 (UTC-4 to UTC-3).
        let zone = chrono_tz::America::Santiago;
        let window = window_in(&zone.with_ymd_and_hms(2026, 9, 10, 12, 0, 0).unwrap()).unwrap();
        let jump = window
            .days
            .iter()
            .find(|d| d.label == "2026-09-06")
            .unwrap();
        assert_eq!(whole_minute("2026-09-06T04:00:00Z"), Some(jump.start));
        assert_eq!(length_in_minutes(&window, "2026-09-06"), 23 * 60);
    }

    #[test]
    fn an_offset_that_is_not_a_whole_minute_gives_no_days() {
        use chrono::TimeZone;
        let zone = chrono::FixedOffset::east_opt(30).unwrap();
        assert!(window_in(&zone.with_ymd_and_hms(2026, 11, 18, 12, 0, 0).unwrap()).is_none());
    }

    #[test]
    fn the_window_reaches_back_for_last_30_days_on_the_first_of_march() {
        use chrono::TimeZone;
        let now = chrono::Utc.with_ymd_and_hms(2027, 3, 1, 12, 0, 0).unwrap();
        let window = window_in(&now).unwrap();
        assert_eq!(window.days.first().unwrap().label, "2027-01-31");
    }

    #[test]
    fn the_clock_of_this_computer_gives_the_days_of_every_period() {
        let calendar = Calendar::default();
        for period in [
            Period::Today,
            Period::ThisWeek,
            Period::LastWeek,
            Period::ThisMonth,
            Period::LastMonth,
            Period::Last7Days,
            Period::Last30Days,
        ] {
            assert!(calendar.days_of(period).is_ok(), "{period:?}");
        }
    }

    #[test]
    fn the_periods_are_whole_calendar_ranges_that_start_on_monday() {
        // 2026-11-18 is a Wednesday.
        let today = date("2026-11-18");
        assert_eq!(today.weekday(), Weekday::Wednesday);
        let of = |p| {
            let (a, b) = bounds(today, p);
            (a.to_string(), b.to_string())
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
            (a.to_string(), b.to_string()),
            ("2027-12-01".into(), "2027-12-31".into())
        );
        let (a, b) = bounds(date("2028-03-02"), Period::LastMonth);
        assert_eq!(
            (a.to_string(), b.to_string()),
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
