//! What a paired phone copies from the computer: the facts, the chats and the dashboard numbers,
//! and a cheap check whether any of them changed.
//!
//! **Format.** Every answer to a command that lists (every one but the check and the document) is
//! `{"ok": true, "format": 1, "token": "...", "items": [...], "next": n}`. Each item is an object with a `kind`. A reader keeps the kinds it knows and
//! skips any other kind, so a later version can add kinds without breaking an older phone.
//! `format` is the version of this format. It is not the protocol number of the channel, which
//! changes only for what an old phone cannot read. The kinds of format 1 are `fact`, `chat`,
//! `message`, `tool` and `numbers`.
//!
//! **Commands**, each one JSON line, after the phone's token (see the module above):
//!
//! * `sync-check` answers `{"ok": true, "format": 1, "tokens": {"facts", "chats", "dashboard"}}`.
//!   A token is an opaque text that changes when its part changes; a part that cannot be read now
//!   has `null`. Nothing is copied by a check, and the numbers are not counted.
//! * `sync-facts`: the facts, as `fact` items with `id`, `entity_kind`, `property`, `value`,
//!   `time` and, when the gear gives one, `tool`. `value` is a string or an object, as the gear
//!   has it; a value that had to be cut is a string (its JSON text) and the item has `"cut": true`,
//!   so a reader takes either type.
//! * `sync-chats`: one `chat` item per chat started on this computer (`id`, `title`, `started`,
//!   `count` of its items); `sync-chat` with an `id` gives that chat's `message` items (`role`,
//!   `time`, `text`) and `tool` items (`time`, `line`). The hidden `recall_facts` calls are not
//!   in a chat.
//! * `sync-document`: the dashboard `document` and the `zone` (the name of the computer's time
//!   zone), on one line of their own.
//! * `sync-dashboard`: each `numbers` item is the answer of `dashboard-numbers` to its `input` (a
//!   widget and days with a label and a start and an end in UTC), for one day or one week
//!   (Monday to Sunday) that has activity.
//!
//! Every command but the check and the document takes `after`, the `next` of the page before,
//! and answers a page of about [`BUDGET`] bytes (and at least one item). `next` is a position in
//! a list that is built again on each call, so a phone sends the `token` of the first page with
//! every later request: when the part changed meanwhile the answer is `"changed": true` with no
//! items, and the phone starts that part again. One item is never split, and no item is over
//! [`ITEM_MAX`] bytes (and a `"cut": true`), so every line fits the channel's limit: a text longer than [`TEXT_LIMIT`]
//! characters (a fact's `value` too, as its JSON text) is cut further when its bytes are still
//! too many, and a cut item carries `"cut": true`.
//!
//! **Numbers.** A day, and a week, are counted once and the answer is kept until the activity
//! around it changes (the minutes of the sessions in it and just beside it, the names of the
//! projects, the document), so a check that finds nothing new counts nothing. A page counts for
//! at most [`PAGE_TIME`] and answers what it has counted; the phone asks again with `next`. The
//! newest days come first.
//!
//! No fact, text of a chat or number is in a log or in an error: failures are fixed words.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{Datelike, NaiveDate, TimeZone};
use connectors::{Record, Role};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::activity::rules::{REACH, SessionMinutes};
use crate::activity::screen_time;
use crate::dashboard::Source;
use crate::dashboard_numbers::{self, Data as Activity};

/// The version of the format of the items.
pub const FORMAT: u64 = 1;
/// About how many bytes of items one answer holds.
pub const BUDGET: usize = 12 * 1024;
/// The most characters of one text of a message or of a fact's value kept whole.
pub const TEXT_LIMIT: usize = 4000;
/// How long one page counts numbers before it answers what it has.
pub const PAGE_TIME: Duration = Duration::from_secs(5);
/// The most answers kept; past it the oldest are dropped by clearing the cache.
const CACHE_LIMIT: usize = 50_000;

/// Why a part cannot be read now: a fixed text, never the data.
pub type Reason = &'static str;

/// The facts of the person as the gear gives them.
#[derive(Clone, Debug, Default)]
pub struct FactsRead {
    /// The version of the profile; the natural token. `None` for a person without a profile.
    pub version: Option<u64>,
    /// The facts, each as the gear lists it: `fact_id`, `entity_kind`, `property`, `value`,
    /// `stored_at` and, when it has one, `tool`.
    pub facts: Vec<Value>,
}

/// One chat, already in the items of the format.
#[derive(Clone, Debug)]
pub struct ChatRead {
    pub id: String,
    pub title: String,
    pub started: String,
    pub items: Vec<Value>,
}

/// Where the engine reads from. The agent's own is [`super::live::Live`]; the test program serves
/// a file.
pub trait Data: Send + Sync {
    /// The facts.
    ///
    /// # Errors
    /// With a fixed reason when they cannot be read now.
    fn facts(&self) -> Result<FactsRead, Reason>;

    /// Only the version of the profile; reads the facts when there is no cheaper way.
    ///
    /// # Errors
    /// With a fixed reason when it cannot be read now.
    fn facts_version(&self) -> Result<Option<u64>, Reason> {
        self.facts().map(|f| f.version)
    }

    /// Every chat, the newest first.
    ///
    /// # Errors
    /// With a fixed reason when they cannot be read now.
    fn chats(&self) -> Result<Vec<ChatRead>, Reason>;

    /// The dashboard document; `None` when there is none.
    ///
    /// # Errors
    /// With a fixed reason when it cannot be read now.
    fn document(&self) -> Result<Option<Value>, Reason>;

    /// What the numbers of `source` count.
    ///
    /// # Errors
    /// With a fixed reason when the activity cannot be read now.
    fn activity(&self, source: Source) -> Result<Activity, Reason>;
}

/// The days of a time zone: which date a minute falls on and where a date starts.
pub trait Calendar: Send + Sync {
    /// The IANA name of the zone, such as `Europe/Berlin`.
    fn name(&self) -> String;
    /// The date of a minute (since the epoch, UTC) in the zone.
    fn date_of(&self, minute: i64) -> Option<NaiveDate>;
    /// The first minute (since the epoch, UTC) of a date in the zone. A date whose midnight does
    /// not exist (a clock change at midnight) starts at the first moment that does.
    fn start_of(&self, date: NaiveDate) -> Option<i64>;
}

/// A time zone of `chrono` with its name.
pub struct Zone<Tz>(pub Tz, pub String);

impl<Tz: TimeZone + Send + Sync> Calendar for Zone<Tz> {
    fn name(&self) -> String {
        self.1.clone()
    }

    fn date_of(&self, minute: i64) -> Option<NaiveDate> {
        let utc = chrono::Utc
            .timestamp_opt(minute.checked_mul(60)?, 0)
            .single()?;
        Some(utc.with_timezone(&self.0).date_naive())
    }

    fn start_of(&self, date: NaiveDate) -> Option<i64> {
        (0..24 * 60).find_map(|m| {
            let naive = date.and_hms_opt(m / 60, m % 60, 0)?;
            let moment = self.0.from_local_datetime(&naive).earliest()?;
            Some(moment.timestamp().div_euclid(60))
        })
    }
}

/// The computer's own zone. Its name is read from `TZ` or from where `/etc/localtime` points; when
/// neither names one, it is `local`.
#[must_use]
pub fn local_calendar() -> Arc<dyn Calendar> {
    let name = std::env::var("TZ")
        .ok()
        .map(|tz| tz.trim_start_matches(':').to_owned())
        .filter(|tz| tz.contains('/'))
        .or_else(|| {
            let target = std::fs::read_link("/etc/localtime").ok()?;
            let target = target.to_str()?;
            Some(target.split_once("zoneinfo/")?.1.to_owned())
        })
        .unwrap_or_else(|| "local".to_owned());
    Arc::new(Zone(chrono::Local, name))
}

/// Whether `command` is one of the sync commands.
#[must_use]
pub fn handles(command: &str) -> bool {
    matches!(
        command,
        "sync-check"
            | "sync-facts"
            | "sync-chats"
            | "sync-chat"
            | "sync-document"
            | "sync-dashboard"
    )
}

/// The items of a chat from its stored records: the turns and the tool lines. The hidden
/// `recall_facts` calls, and every result of a tool, are left out. `line` gives the text of a
/// tool call.
#[must_use]
pub fn chat_items(records: &[Record], line: &dyn Fn(&str, &str) -> String) -> Vec<Value> {
    records
        .iter()
        .filter_map(|record| match record {
            Record::Turn(t) => Some(json!({
                "kind": "message",
                "role": match t.role { Role::User => "user", Role::Assistant => "assistant" },
                "time": t.time,
                "text": t.text,
            })),
            Record::ToolCall(c) if c.name != chat_core::recall::NAME => Some(json!({
                "kind": "tool",
                "time": c.time,
                "line": line(&c.name, &c.arguments),
            })),
            _ => None,
        })
        .collect()
}

/// One stored chat in the items of the format; `None` for a chat with no record.
#[must_use]
pub fn chat_read(
    id: String,
    records: &[Record],
    line: &dyn Fn(&str, &str) -> String,
) -> Option<ChatRead> {
    let started = records.first()?.time().to_owned();
    let title = records
        .iter()
        .find_map(|r| match r {
            Record::Turn(t) if t.role == Role::User => {
                Some(cut_chars(&t.text, chat_core::chat::TITLE_CHARS))
            }
            _ => None,
        })
        .unwrap_or_default();
    Some(ChatRead {
        id,
        title,
        started,
        items: chat_items(records, line),
    })
}

/// What an old reader does with an item, as a reference for the phone's reader and the tests: its
/// kind when it is one of format 1, `None` otherwise.
#[must_use]
#[doc(hidden)]
pub fn known_kind(item: &Value) -> Option<&str> {
    item.get("kind")
        .and_then(Value::as_str)
        .filter(|k| matches!(*k, "fact" | "chat" | "message" | "tool" | "numbers"))
}

fn digest(parts: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part);
    }
    hex::encode(hash.finalize())
}

fn token(part: &str, basis: &str) -> String {
    digest(&[
        b"sync",
        &FORMAT.to_le_bytes(),
        part.as_bytes(),
        basis.as_bytes(),
    ])
}

/// The most bytes of one item, before the eleven of its `"cut": true`. A line is at most
/// `MAX_LINE` bytes and an answer adds about a hundred of its own, so an item of this size fits one.
pub const ITEM_MAX: usize = 14 * 1024;

/// The keys whose text may be cut. A fact's `value` is one of them: a value that is an object is
/// cut as its JSON text and then is a string.
const CUTTABLE: [&str; 4] = ["text", "line", "title", "value"];

fn cut_chars(text: &str, keep: usize) -> String {
    text.chars().take(keep).collect()
}

/// An item that fits one line. A long text is cut at [`TEXT_LIMIT`] characters, and what is still
/// over [`ITEM_MAX`] bytes (characters that serialise long, or a big answer of numbers) is cut
/// further. A cut item carries `"cut": true`. An item with nothing to cut that is still too big
/// is reduced to its kind and `"cut": true`.
fn limited(mut item: Value) -> Value {
    let mut was_cut = false;
    let Some(object) = item.as_object_mut() else {
        return item;
    };
    for key in CUTTABLE {
        let text = match object.get(key) {
            Some(Value::String(text)) => text.clone(),
            Some(other) if key == "value" => other.to_string(),
            _ => continue,
        };
        let is_text = matches!(object.get(key), Some(Value::String(_)));
        if text.chars().count() > TEXT_LIMIT || (!is_text && text.len() > ITEM_MAX) {
            object.insert(key.to_owned(), Value::String(cut_chars(&text, TEXT_LIMIT)));
            was_cut = true;
        }
    }
    while Value::Object(object.clone()).to_string().len() > ITEM_MAX {
        was_cut = true;
        // The longest text halves; with none left, groups of numbers go; then the item is a stub.
        let longest = CUTTABLE
            .iter()
            .filter_map(|k| match object.get(*k) {
                Some(Value::String(t)) if t.len() > 16 => Some((t.len(), *k)),
                _ => None,
            })
            .max();
        if let Some((_, key)) = longest {
            if let Some(Value::String(text)) = object.get(key) {
                let half = cut_chars(text, text.chars().count() / 2);
                object.insert(key.to_owned(), Value::String(half));
            }
        } else if let Some(Value::Array(groups)) = object
            .get_mut("answer")
            .and_then(|a| a.get_mut("groups"))
            .filter(|g| g.as_array().is_some_and(|g| !g.is_empty()))
        {
            groups.pop();
        } else {
            let kind = object.get("kind").cloned().unwrap_or(Value::Null);
            object.clear();
            object.insert("kind".to_owned(), kind);
            break;
        }
    }
    if was_cut {
        object.insert("cut".to_owned(), Value::Bool(true));
    }
    item
}

fn fact_item(raw: &Value) -> Option<Value> {
    let text = |key: &str| raw.get(key).and_then(Value::as_str);
    let mut item = json!({
        "kind": "fact",
        "id": text("fact_id")?,
        "entity_kind": text("entity_kind")?,
        "property": text("property")?,
        "value": raw.get("value")?,
        "time": text("stored_at")?,
    });
    if let Some(tool) = text("tool") {
        item["tool"] = json!(tool);
    }
    Some(item)
}

/// What a request asks, read: the command's `after`, and a chat's `id`.
struct Ask {
    after: usize,
    id: Option<String>,
    token: Option<String>,
}

fn read_ask(request: &Value) -> Option<Ask> {
    let object = request.as_object()?;
    if object
        .keys()
        .any(|k| !matches!(k.as_str(), "cmd" | "after" | "id" | "token"))
    {
        return None;
    }
    let after = match object.get("after") {
        None | Some(Value::Null) => 0,
        Some(v) => usize::try_from(v.as_u64()?).ok()?,
    };
    let id = match object.get("id") {
        None => None,
        Some(v) => Some(v.as_str()?.to_owned()),
    };
    let token = match object.get("token") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_str()?.to_owned()),
    };
    Some(Ask { after, id, token })
}

fn failure(text: &str) -> Value {
    json!({ "error": text })
}

fn page(token: &str, items: &[Value], after: usize) -> Value {
    let mut chosen = Vec::new();
    let mut size = 0;
    let mut at = after;
    while let Some(item) = items.get(at) {
        let item = limited(item.clone());
        let length = item.to_string().len();
        if !chosen.is_empty() && size + length > BUDGET {
            break;
        }
        size += length;
        chosen.push(item);
        at += 1;
    }
    let next = (at < items.len()).then_some(at);
    answer(token, &chosen, next)
}

fn answer(token: &str, items: &[Value], next: Option<usize>) -> Value {
    json!({
        "ok": true,
        "format": FORMAT,
        "token": token,
        "items": items,
        "next": next,
    })
}

/// What a page answers when the part changed since the page before: nothing of the new data, and
/// `"changed": true`, so the phone starts that part again from its first page.
fn changed(token: &str) -> Value {
    json!({"ok": true, "format": FORMAT, "changed": true, "token": token, "items": [], "next": null})
}

/// One entry of the dashboard: a widget over a day or a week.
struct Entry {
    widget: String,
    week: bool,
    start: NaiveDate,
    days: Vec<Value>,
    stamp: String,
}

struct Dash {
    document: Option<Value>,
    zone: String,
    entries: Vec<Entry>,
    data: Vec<(Source, Activity)>,
    token: String,
}

/// The days of one source that have activity, each with a stamp of what is in and beside it.
struct Buckets {
    stamps: BTreeMap<NaiveDate, String>,
    names: String,
}

fn session_meta(s: &SessionMinutes) -> [u8; 32] {
    let unattended = [u8::from(s.unattended)];
    let hash = digest(&[
        s.tool.as_bytes(),
        s.session.as_bytes(),
        s.project.as_bytes(),
        s.ticket.as_bytes(),
        s.area.as_bytes(),
        s.collection.as_bytes(),
        &unattended,
    ]);
    let mut out = [0; 32];
    out.copy_from_slice(&hex::decode(hash).unwrap_or_default());
    out
}

struct Hashers {
    own: BTreeSet<NaiveDate>,
    by_date: HashMap<NaiveDate, (Sha256, Option<usize>)>,
    bounds: HashMap<NaiveDate, Option<(i64, i64)>>,
}

impl Hashers {
    fn bounds(&mut self, calendar: &dyn Calendar, date: NaiveDate) -> Option<(i64, i64)> {
        *self.bounds.entry(date).or_insert_with(|| {
            Some((
                calendar.start_of(date)?,
                calendar.start_of(date.succ_opt()?)?,
            ))
        })
    }

    fn feed(&mut self, date: NaiveDate, owner: usize, meta: &[u8], bytes: &[u8]) {
        let (hash, last) = self
            .by_date
            .entry(date)
            .or_insert_with(|| (Sha256::new(), None));
        if *last != Some(owner) {
            hash.update(meta);
            *last = Some(owner);
        }
        hash.update(bytes);
    }

    /// An event at `minute`: it belongs to its own date, and to a neighbour it is close to.
    fn event(
        &mut self,
        calendar: &dyn Calendar,
        minute: i64,
        owner: usize,
        meta: &[u8],
        bytes: &[u8],
    ) {
        let Some(date) = calendar.date_of(minute) else {
            return;
        };
        self.own.insert(date);
        self.feed(date, owner, meta, bytes);
        let Some((start, end)) = self.bounds(calendar, date) else {
            return;
        };
        if minute < start + REACH
            && let Some(before) = date.pred_opt()
        {
            self.feed(before, owner, meta, bytes);
        }
        if minute >= end - REACH
            && let Some(after) = date.succ_opt()
        {
            self.feed(after, owner, meta, bytes);
        }
    }
}

fn buckets(data: &Activity, source: Source, calendar: &dyn Calendar) -> Buckets {
    let mut hashers = Hashers {
        own: BTreeSet::new(),
        by_date: HashMap::new(),
        bounds: HashMap::new(),
    };
    // Projects are named over all the sessions, so the set of them is part of every day.
    let mut names = BTreeSet::new();
    for s in &data.sessions {
        names.insert((s.project.as_str(), s.area.as_str(), s.collection.as_str()));
    }
    let names = digest(
        &names
            .iter()
            .flat_map(|(a, b, c)| [a.as_bytes(), b.as_bytes(), c.as_bytes()])
            .collect::<Vec<_>>(),
    );
    match source {
        Source::AiSessions => {
            for (owner, s) in data.sessions.iter().enumerate() {
                let meta = session_meta(s);
                for minute in &s.minutes {
                    hashers.event(calendar, *minute, owner, &meta, &minute.to_le_bytes());
                }
            }
        }
        Source::OnScreen => {
            for (owner, block) in data.blocks.iter().enumerate() {
                let Some(start) = screen_time::epoch(&block.start) else {
                    continue;
                };
                let bytes = digest(&[
                    block.start.as_bytes(),
                    &block.seconds.to_le_bytes(),
                    block.app.as_bytes(),
                    block.site.as_bytes(),
                    block.project.as_bytes(),
                    block.ticket.as_bytes(),
                ]);
                let first = start.div_euclid(60);
                let seconds = i64::try_from(block.seconds).unwrap_or(0);
                let last = (start + seconds).div_euclid(60);
                hashers.event(calendar, first, owner, &[], bytes.as_bytes());
                if last != first {
                    hashers.event(calendar, last, owner, &[], bytes.as_bytes());
                }
            }
        }
    }
    let stamps = hashers
        .own
        .iter()
        .filter_map(|date| {
            let (hash, _) = hashers.by_date.remove(date)?;
            Some((*date, hex::encode(hash.finalize())))
        })
        .collect();
    Buckets { stamps, names }
}

fn utc_minute(minute: i64) -> Option<String> {
    let at = chrono::DateTime::from_timestamp(minute.checked_mul(60)?, 0)?;
    Some(at.format("%Y-%m-%dT%H:%M:00Z").to_string())
}

fn day_input(calendar: &dyn Calendar, date: NaiveDate) -> Option<Value> {
    let start = calendar.start_of(date)?;
    let end = calendar.start_of(date.succ_opt()?)?;
    Some(json!({
        "label": date.format("%Y-%m-%d").to_string(),
        "start": utc_minute(start)?,
        "end": utc_minute(end)?,
    }))
}

fn monday_of(date: NaiveDate) -> NaiveDate {
    date - chrono::Duration::days(i64::from(date.weekday().num_days_from_monday()))
}

/// The reader of the sync commands over one [`Data`].
pub struct Engine {
    data: Arc<dyn Data>,
    calendar: Arc<dyn Calendar>,
    cache: Mutex<HashMap<String, (String, Value)>>,
    counted: std::sync::atomic::AtomicUsize,
    page_time: Duration,
}

impl Engine {
    #[must_use]
    pub fn new(data: Arc<dyn Data>, calendar: Arc<dyn Calendar>) -> Self {
        Self {
            data,
            calendar,
            cache: Mutex::new(HashMap::new()),
            counted: std::sync::atomic::AtomicUsize::new(0),
            page_time: PAGE_TIME,
        }
    }

    /// Count for `time` in one page instead of [`PAGE_TIME`].
    #[doc(hidden)]
    #[must_use]
    pub fn with_page_time(mut self, time: Duration) -> Self {
        self.page_time = time;
        self
    }

    /// How many answers of the numbers were counted, not taken from what was kept.
    #[must_use]
    pub fn counted(&self) -> usize {
        self.counted.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The answer to a sync command. It may read and count for seconds: call it where blocking
    /// is fine.
    #[must_use]
    pub fn answer(&self, request: &Value) -> Value {
        let Some(command) = request.get("cmd").and_then(Value::as_str) else {
            return failure("unknown command");
        };
        let Some(ask) = read_ask(request) else {
            return failure("invalid sync request");
        };
        let reply = match command {
            "sync-check" if ask.after == 0 && ask.id.is_none() && ask.token.is_none() => {
                self.check()
            }
            "sync-facts" if ask.id.is_none() => self.facts(&ask),
            "sync-chats" if ask.id.is_none() => self.chats(&ask),
            "sync-chat" if ask.id.is_some() => self.chat(&ask),
            "sync-document" if ask.id.is_none() && ask.after == 0 => self.document(),
            "sync-dashboard" if ask.id.is_none() => self.dashboard(&ask),
            _ => failure("invalid sync request"),
        };
        match (&ask.token, reply.get("token").and_then(Value::as_str)) {
            (Some(asked), Some(now)) if asked != now && reply.get("items").is_some() => {
                changed(now)
            }
            _ => reply,
        }
    }

    fn check(&self) -> Value {
        let facts = self.facts_token().map_or(Value::Null, Value::String);
        let chats = self
            .data
            .chats()
            .map_or(Value::Null, |c| Value::String(Self::chats_token(&c)));
        let dashboard = self.plan().map_or(Value::Null, |d| Value::String(d.token));
        json!({
            "ok": true,
            "format": FORMAT,
            "tokens": { "facts": facts, "chats": chats, "dashboard": dashboard },
        })
    }

    fn facts_token(&self) -> Result<String, Reason> {
        // A profile with no version has no natural token: the facts themselves are the basis.
        if let Some(version) = self.data.facts_version()? {
            return Ok(token("facts", &format!("v{version}")));
        }
        let read = self.data.facts()?;
        Ok(token("facts", &Self::facts_basis(&read)))
    }

    fn facts_basis(read: &FactsRead) -> String {
        match read.version {
            Some(version) => format!("v{version}"),
            None => digest(
                &read
                    .facts
                    .iter()
                    .filter_map(fact_item)
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .iter()
                    .map(String::as_bytes)
                    .collect::<Vec<_>>(),
            ),
        }
    }

    fn facts(&self, ask: &Ask) -> Value {
        let Ok(read) = self.data.facts() else {
            return failure("facts unavailable");
        };
        let token = token("facts", &Self::facts_basis(&read));
        let items: Vec<Value> = read.facts.iter().filter_map(fact_item).collect();
        page(&token, &items, ask.after)
    }

    fn chats_token(chats: &[ChatRead]) -> String {
        let all: Vec<String> = chats
            .iter()
            .map(|c| json!([c.id, c.title, c.started, c.items]).to_string())
            .collect();
        token(
            "chats",
            &digest(&all.iter().map(String::as_bytes).collect::<Vec<_>>()),
        )
    }

    fn chats(&self, ask: &Ask) -> Value {
        let Ok(chats) = self.data.chats() else {
            return failure("chats unavailable");
        };
        let items: Vec<Value> = chats
            .iter()
            .map(|c| {
                json!({
                    "kind": "chat",
                    "id": c.id,
                    "title": c.title,
                    "started": c.started,
                    "count": c.items.len(),
                })
            })
            .collect();
        page(&Self::chats_token(&chats), &items, ask.after)
    }

    fn chat(&self, ask: &Ask) -> Value {
        let Ok(chats) = self.data.chats() else {
            return failure("chats unavailable");
        };
        let token = Self::chats_token(&chats);
        let Some(chat) = chats.into_iter().find(|c| Some(&c.id) == ask.id.as_ref()) else {
            return failure("no such chat");
        };
        page(&token, &chat.items, ask.after)
    }

    /// The entries of the dashboard, with the stamps of their activity and nothing counted.
    fn plan(&self) -> Result<Dash, Reason> {
        let calendar = self.calendar.as_ref();
        let zone = calendar.name();
        let Some(document) = self.data.document()? else {
            return Ok(Dash {
                token: token("dashboard", "none"),
                document: None,
                zone,
                entries: Vec::new(),
                data: Vec::new(),
            });
        };
        let payload = dashboard_numbers::payload_of(Some(&document))
            .map_err(|_| "dashboard settings unreadable")?;
        let salt = digest(&[document.to_string().as_bytes(), zone.as_bytes()]);
        let widgets: Vec<(String, Source)> = payload
            .widgets
            .iter()
            .map(|w| (w.id.clone(), dashboard_numbers::spec_of(w, &payload).source))
            .collect();
        let mut data: Vec<(Source, Activity)> = Vec::new();
        let mut spread: Vec<(Source, Buckets)> = Vec::new();
        for (_, source) in &widgets {
            if data.iter().all(|(s, _)| s != source) {
                let read = self.data.activity(*source)?;
                spread.push((*source, buckets(&read, *source, calendar)));
                data.push((*source, read));
            }
        }
        let mut entries = Vec::new();
        for (widget, source) in &widgets {
            let Some((_, found)) = spread.iter().find(|(s, _)| s == source) else {
                continue;
            };
            let mut mondays = BTreeSet::new();
            for (date, stamp) in &found.stamps {
                mondays.insert(monday_of(*date));
                let Some(input) = day_input(calendar, *date) else {
                    continue;
                };
                entries.push(Entry {
                    widget: widget.clone(),
                    week: false,
                    start: *date,
                    days: vec![input],
                    stamp: digest(&[
                        salt.as_bytes(),
                        widget.as_bytes(),
                        found.names.as_bytes(),
                        stamp.as_bytes(),
                    ]),
                });
            }
            for monday in mondays {
                let dates: Vec<NaiveDate> =
                    (0..7).map(|d| monday + chrono::Duration::days(d)).collect();
                let days: Option<Vec<Value>> =
                    dates.iter().map(|d| day_input(calendar, *d)).collect();
                let Some(days) = days else { continue };
                let mut parts = vec![salt.as_bytes(), widget.as_bytes(), found.names.as_bytes()];
                for date in &dates {
                    parts.push(found.stamps.get(date).map_or("", String::as_str).as_bytes());
                }
                entries.push(Entry {
                    widget: widget.clone(),
                    week: true,
                    start: monday,
                    days,
                    stamp: digest(&parts),
                });
            }
        }
        // The newest first; a week stands before its own first day.
        entries.sort_by(|a, b| (b.start, b.week, &a.widget).cmp(&(a.start, a.week, &b.widget)));
        let stamps: Vec<String> = entries
            .iter()
            .map(|e| format!("{}|{}|{}", e.widget, e.week, e.stamp))
            .collect();
        let token = token(
            "dashboard",
            &digest(&stamps.iter().map(String::as_bytes).collect::<Vec<_>>()),
        );
        Ok(Dash {
            document: Some(document),
            zone,
            entries,
            data,
            token,
        })
    }

    fn numbers_item(&self, dash: &Dash, entry: &Entry) -> Value {
        let input = json!({ "widget": entry.widget, "days": entry.days });
        let key = format!(
            "{}|{}|{}",
            entry.widget,
            if entry.week { "week" } else { "day" },
            entry.start
        );
        let cached = {
            let cache = crate::sync::lock(&self.cache);
            cache
                .get(&key)
                .filter(|(stamp, _)| *stamp == entry.stamp)
                .map(|(_, answer)| answer.clone())
        };
        let answer = cached.unwrap_or_else(|| {
            self.counted
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let line =
                dashboard_numbers::respond(&input.to_string(), dash.document.as_ref(), |source| {
                    dash.data
                        .iter()
                        .find(|(s, _)| *s == source)
                        .map(|(_, d)| d.clone())
                        .ok_or("the activity time could not be read")
                });
            let answer: Value =
                serde_json::from_str(&line).unwrap_or_else(|_| failure("numbers unavailable"));
            let mut cache = crate::sync::lock(&self.cache);
            if cache.len() >= CACHE_LIMIT {
                cache.clear();
            }
            cache.insert(key, (entry.stamp.clone(), answer.clone()));
            answer
        });
        json!({
            "kind": "numbers",
            "period": if entry.week { "week" } else { "day" },
            "widget": entry.widget,
            "input": input,
            "answer": answer,
        })
    }

    /// The dashboard document and the zone, on a page of their own: the document is not counted
    /// in the budget of a page of items.
    fn document(&self) -> Value {
        let Ok(dash) = self.plan() else {
            return failure("dashboard unavailable");
        };
        let document = dash.document.clone().unwrap_or(Value::Null);
        if document.to_string().len() > ITEM_MAX {
            return failure("dashboard document too large");
        }
        json!({
            "ok": true,
            "format": FORMAT,
            "token": dash.token,
            "document": document,
            "zone": dash.zone,
        })
    }

    fn dashboard(&self, ask: &Ask) -> Value {
        let Ok(dash) = self.plan() else {
            return failure("dashboard unavailable");
        };
        // Nothing is counted for a copy that is already out of date.
        if ask.token.as_deref().is_some_and(|t| t != dash.token) {
            return changed(&dash.token);
        }
        let began = Instant::now();
        let mut items = Vec::new();
        let mut size = 0;
        let mut at = ask.after;
        while let Some(entry) = dash.entries.get(at) {
            if !items.is_empty() && began.elapsed() >= self.page_time {
                break;
            }
            let item = limited(self.numbers_item(&dash, entry));
            let length = item.to_string().len();
            if !items.is_empty() && size + length > BUDGET {
                break;
            }
            size += length;
            items.push(item);
            at += 1;
        }
        let next = (at < dash.entries.len()).then_some(at);
        answer(&dash.token, &items, next)
    }
}
