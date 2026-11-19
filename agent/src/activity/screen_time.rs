//! The time on screen: how long each application, site, project and ticket was in front, worked
//! out from the screen samples (see [`crate::screen`]).
//!
//! Like [`super::rules`] this is a plain function of its input, with no clock, no model and no
//! network. The rules ride on [`super::RULES_VERSION`]: change one and bump it.
//!
//! * A sample counts for its application and its site while the person is not away, and at most
//!   its own [`SAMPLE_SECONDS`], so a gap between samples adds nothing.
//! * The person is away when the last input is older than [`AWAY_AFTER`] seconds. In a call (a
//!   call window in front, the screen on) they count as present whatever the input says. A
//!   locked or dark screen counts nothing.
//! * The site is the host of the tab address without `www.`.
//! * The ticket is the id in the tab address, then in the tab title, then in the window title:
//!   the first place that holds exactly one id wins (the ids are found by [`assign`]).
//! * The project is a project the dashboard knows, found as a whole word in the address or a
//!   title, when exactly one is found. None or several give none: a project is never guessed.
//! * A private window keeps only the application. A password manager keeps no title.
//! * One application is in front at a time, so the blocks never overlap ([`without_overlap`]).
//!
//! A block never holds a title or an address: only the application, the site, the project and the
//! ticket.

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use url::Url;

use super::assign;
use crate::screen::{Line, Tab};

/// The time a sample counts for: the interval between two samples.
pub const SAMPLE_SECONDS: u64 = 5;
/// The person is away when the last input is older than this many seconds.
pub const AWAY_AFTER: u64 = 120;

/// The bundle ids of the call applications, in lower case. A call window of one of them in front
/// is a call.
pub const CALL_APPS: [&str; 6] = [
    "us.zoom.xos",
    "com.microsoft.teams",
    "com.microsoft.teams2",
    "com.cisco.webexmeetingsapp",
    "com.webex.meetingmanager",
    "com.apple.facetime",
];
/// The hosts of the meeting sites, in lower case; their subdomains count too. A tab on one of
/// them in front is a call.
pub const MEETING_SITES: [&str; 1] = ["meet.google.com"];
/// The start of the bundle ids of the password managers, in lower case. They keep no title.
pub const PASSWORD_MANAGERS: [&str; 9] = [
    "com.1password.",
    "com.agilebits.",
    "com.bitwarden.",
    "com.dashlane.",
    "com.lastpass.",
    "org.keepassxc.",
    "in.sinew.enpass",
    "com.apple.keychainaccess",
    "com.apple.passwords",
];

/// The time one application was in front, from one run of samples.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// The time of the first sample, as the run has it: the key of the block.
    pub start: String,
    pub seconds: u64,
    pub app: String,
    /// Empty when the front window is not a tab, or the tab is private.
    pub site: String,
    pub project: String,
    pub ticket: String,
}

/// The seconds between two times, `None` when either is not RFC 3339.
pub(crate) fn epoch(time: &str) -> Option<i64> {
    OffsetDateTime::parse(time, &Rfc3339)
        .ok()
        .map(OffsetDateTime::unix_timestamp)
}

/// The host of an address without `www.`, for the addresses of web pages only.
fn site_of(address: &str) -> String {
    let Ok(url) = Url::parse(address) else {
        return String::new();
    };
    if !matches!(url.scheme(), "http" | "https") {
        return String::new();
    }
    let host = url.host_str().unwrap_or_default().to_lowercase();
    host.strip_prefix("www.").unwrap_or(&host).to_owned()
}

fn on_meeting_site(site: &str) -> bool {
    MEETING_SITES
        .iter()
        .any(|m| site == *m || site.ends_with(&format!(".{m}")))
}

fn starts_with_any(bundle: &str, starts: &[&str]) -> bool {
    starts.iter().any(|s| bundle.starts_with(s))
}

/// The one id a place holds, none when it holds none or several.
fn ticket_in(text: &str) -> Option<String> {
    let mut found = assign::tickets_in_text(text);
    found.sort();
    found.dedup();
    match found.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_'
}

/// Whether `name` stands in `text` as a whole word, ignoring case.
fn names(text: &str, name: &str) -> bool {
    let (text, name) = (text.to_lowercase(), name.to_lowercase());
    text.match_indices(&name).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + name.len()..].chars().next();
        !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
    })
}

/// The one known project the places name, none when they name none or several.
fn project_in(places: &[&str], projects: &[String]) -> Option<String> {
    let mut found: Vec<&String> = projects
        .iter()
        .filter(|p| !p.is_empty() && places.iter().any(|text| names(text, p)))
        .collect();
    found.sort();
    found.dedup();
    match found.as_slice() {
        [one] => Some((*one).clone()),
        _ => None,
    }
}

/// The seconds of a run that count: its samples at [`SAMPLE_SECONDS`] each, never more than the
/// run spans, and only while the person is not away. The idle time of a run grows as the clock
/// does (a run ends at the next input), so the first sample's idle time says when the person
/// went away.
fn counted_seconds(line: &Line, call: bool) -> Option<u64> {
    let span = u64::try_from(epoch(&line.until)? - epoch(&line.time)?).unwrap_or(0);
    let covered = line
        .samples
        .saturating_mul(SAMPLE_SECONDS)
        .min(span + SAMPLE_SECONDS);
    let reading = &line.reading;
    if reading.locked {
        return Some(0);
    }
    if call {
        return Some(covered);
    }
    // The last sample that is not away is at most this far into the run, and counts its own time.
    let present = AWAY_AFTER.checked_sub(reading.idle_seconds)? + SAMPLE_SECONDS;
    Some(covered.min(present))
}

/// The block a run of samples makes, with the projects the dashboard knows (by name). `None`
/// when it counts no time: the screen is locked, the person is away, nothing is in front, or the
/// run has no usable times.
#[must_use]
pub fn block_of(line: &Line, projects: &[String]) -> Option<Block> {
    let reading = &line.reading;
    let app = reading.app.as_ref()?;
    let bundle = app.bundle_id.to_lowercase();
    let tab = reading.tab.as_ref();
    let private = tab.is_some_and(|t| t.private);
    let site = match tab {
        Some(Tab {
            url,
            private: false,
            ..
        }) => site_of(url),
        _ => String::new(),
    };
    let call = reading.call || starts_with_any(&bundle, &CALL_APPS) || on_meeting_site(&site);
    let seconds = counted_seconds(line, call).filter(|s| *s > 0)?;
    let mut block = Block {
        start: line.time.clone(),
        seconds,
        app: app.name.clone(),
        site,
        project: String::new(),
        ticket: String::new(),
    };
    if private {
        return Some(block);
    }
    let titled = !starts_with_any(&bundle, &PASSWORD_MANAGERS);
    let mut places: Vec<&str> = Vec::new();
    if let Some(tab) = tab {
        places.push(&tab.url);
        if titled {
            places.push(&tab.title);
        }
    }
    if titled && let Some(title) = &reading.window_title {
        places.push(title);
    }
    block.ticket = places.iter().find_map(|t| ticket_in(t)).unwrap_or_default();
    block.project = project_in(&places, projects).unwrap_or_default();
    Some(block)
}

/// The blocks in order of time, each cut where the next one starts, so that no time is counted
/// twice. A block with no time left, or without a usable start, is dropped.
#[must_use]
pub fn without_overlap(blocks: Vec<Block>) -> Vec<Block> {
    let mut timed: Vec<(i64, Block)> = blocks
        .into_iter()
        .filter_map(|b| Some((epoch(&b.start)?, b)))
        .collect();
    timed.sort_by(|a, b| (a.0, &a.1.start).cmp(&(b.0, &b.1.start)));
    let starts: Vec<i64> = timed.iter().map(|(at, _)| *at).collect();
    timed
        .into_iter()
        .enumerate()
        .filter_map(|(i, (at, mut block))| {
            if let Some(next) = starts.get(i + 1) {
                block.seconds = block.seconds.min(u64::try_from(next - at).unwrap_or(0));
            }
            (block.seconds > 0).then_some(block)
        })
        .collect()
}

/// The name a project goes by: the last part of its folder, literally. The label a report shows
/// for two projects of one name has more parts of the path, which no title holds as one word.
#[must_use]
pub fn project_name(folder: &str) -> &str {
    folder
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or_default()
}
