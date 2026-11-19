//! The screen samples: the sampler on a fake clock and fake signals, the file of the samples,
//! the erasure and the `screen` row of the `status` answer, with real settings on a temporary
//! directory.
#![cfg(unix)]
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions use it too

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use construct_agent::erase::{OwnStore, OwnStores};
use construct_agent::ipc::{Server, query_status, request};
use construct_agent::screen::{
    App, Clock, Failure, Grant, GrantState, Reading, Screen, Signals, Tab,
};
use construct_agent::settings::{Settings, Update};
use serde_json::{Value, json};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A directory of its own, with the settings in `config` and the data in `data`.
struct Dirs {
    root: PathBuf,
}

impl Dirs {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "construct-screen-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn settings(&self) -> Settings {
        Settings::open(&self.root.join("config")).unwrap()
    }

    fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    fn screen_dir(&self) -> PathBuf {
        self.data().join("records").join("screen")
    }
}

impl Drop for Dirs {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn set(settings: &Settings, json: &str) {
    settings.apply(Update::parse(json).unwrap()).unwrap();
}

/// Noon of a local day, so that a test of a few hours does not cross local midnight.
fn local_noon() -> SystemTime {
    use chrono::{Local, TimeZone};
    let around =
        chrono::DateTime::<Local>::from(SystemTime::UNIX_EPOCH + Duration::from_hours(500_004));
    let noon = around.date_naive().and_hms_opt(12, 0, 0).unwrap();
    SystemTime::from(Local.from_local_datetime(&noon).earliest().unwrap())
}

/// The first local midnight after `time`.
fn next_local_midnight(time: SystemTime) -> SystemTime {
    use chrono::{Local, TimeZone};
    let day = chrono::DateTime::<Local>::from(time)
        .date_naive()
        .succ_opt()
        .unwrap();
    let midnight = day.and_hms_opt(0, 0, 0).unwrap();
    SystemTime::from(Local.from_local_datetime(&midnight).earliest().unwrap())
}

/// A clock the test moves.
struct FakeClock(Mutex<SystemTime>);

impl FakeClock {
    fn at_noon() -> Arc<Self> {
        Arc::new(Self(Mutex::new(local_noon())))
    }

    fn at(time: SystemTime) -> Arc<Self> {
        Arc::new(Self(Mutex::new(time)))
    }

    fn set(&self, time: SystemTime) {
        *self.0.lock().unwrap() = time;
    }

    fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> SystemTime {
        *self.0.lock().unwrap()
    }
}

/// The longest a fake call may take in the tests: long enough for a loaded machine.
const CALL_LIMIT: Duration = Duration::from_secs(1);

const TITLE: &str = "Quarterly plan - secret title";
const ADDRESS: &str = "https://example.org/private/path?token=abc";

fn reading(idle_seconds: u64) -> Reading {
    Reading {
        app: Some(App {
            name: "Browser".into(),
            bundle_id: "org.example.browser".into(),
        }),
        window_title: Some(TITLE.into()),
        tab: Some(Tab {
            title: TITLE.into(),
            url: ADDRESS.into(),
            private: true,
        }),
        idle_seconds,
        call: false,
        locked: false,
    }
}

/// Signals the test drives: the answers are queued, and the last one repeats.
struct FakeSignals {
    supported: bool,
    grants: Mutex<Vec<Grant>>,
    /// Reads that do not return until their receiver gets something, one for each next read.
    hangs: Mutex<Vec<Receiver<()>>>,
    last: Mutex<Result<Reading, Failure>>,
    reads: AtomicUsize,
    /// Runs in the middle of every read, before it answers.
    during_read: Mutex<Option<Box<dyn Fn() + Send>>>,
    /// Grants calls that do not return until their receiver gets something.
    hang_grants: Mutex<Vec<Receiver<()>>>,
    /// The next read panics.
    panic_next: AtomicBool,
}

impl FakeSignals {
    fn new(first: Result<Reading, Failure>) -> Arc<Self> {
        Arc::new(Self {
            supported: true,
            grants: Mutex::new(vec![Grant {
                name: "accessibility",
                state: GrantState::Granted,
            }]),
            hangs: Mutex::new(Vec::new()),
            last: Mutex::new(first),
            reads: AtomicUsize::new(0),
            during_read: Mutex::new(None),
            hang_grants: Mutex::new(Vec::new()),
            panic_next: AtomicBool::new(false),
        })
    }

    fn answer(&self, next: Result<Reading, Failure>) {
        *self.last.lock().unwrap() = next;
    }

    fn hang_next(&self, release: Receiver<()>) {
        self.hangs.lock().unwrap().push(release);
    }
}

impl Signals for FakeSignals {
    fn supported(&self) -> bool {
        self.supported
    }

    fn grants(&self) -> Vec<Grant> {
        let hang = {
            let mut hangs = self.hang_grants.lock().unwrap();
            (!hangs.is_empty()).then(|| hangs.remove(0))
        };
        if let Some(release) = hang {
            let _ = release.recv();
        }
        self.grants.lock().unwrap().clone()
    }

    fn read(&self) -> Result<Reading, Failure> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let hang = {
            let mut hangs = self.hangs.lock().unwrap();
            (!hangs.is_empty()).then(|| hangs.remove(0))
        };
        if let Some(release) = hang {
            let _ = release.recv();
        }
        assert!(
            !self.panic_next.swap(false, Ordering::SeqCst),
            "a read that fails"
        );
        if let Some(during) = self.during_read.lock().unwrap().as_ref() {
            during();
        }
        self.last.lock().unwrap().clone()
    }
}

struct Rig {
    dirs: Dirs,
    settings: Settings,
    clock: Arc<FakeClock>,
    signals: Arc<FakeSignals>,
    screen: Screen,
}

fn rig(first: Result<Reading, Failure>) -> Rig {
    let dirs = Dirs::new();
    let settings = dirs.settings();
    let clock = FakeClock::at_noon();
    let signals = FakeSignals::new(first);
    let screen = Screen::with_timing(
        settings.clone(),
        &dirs.data(),
        signals.clone(),
        clock.clone(),
        Duration::from_secs(5),
        CALL_LIMIT,
    );
    Rig {
        dirs,
        settings,
        clock,
        signals,
        screen,
    }
}

/// The lines of every file of the samples.
fn lines(dir: &Path) -> Vec<Value> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .map(|entries| entries.map(|e| e.unwrap().path()).collect())
        .unwrap_or_default();
    files.sort();
    files
        .iter()
        .flat_map(|f| {
            fs::read_to_string(f)
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str::<Value>(l).unwrap())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn samples(lines: &[Value]) -> u64 {
    lines.iter().map(|l| l["samples"].as_u64().unwrap()).sum()
}

#[test]
fn with_the_setting_off_nothing_is_stored_and_the_row_says_off() {
    let rig = rig(Ok(reading(1)));
    for _ in 0..3 {
        rig.screen.tick();
        rig.clock.advance(Duration::from_secs(5));
    }
    assert!(!rig.dirs.screen_dir().exists());
    assert_eq!(rig.signals.reads.load(Ordering::SeqCst), 0);
    let row = rig.screen.status();
    assert_eq!(row["state"], "off");
    assert_eq!(row["reason"], "switched off");
    assert_eq!(row["samples_today"], 0);
}

#[test]
fn the_setting_is_off_whenever_all_collection_is_off() {
    let rig = rig(Ok(reading(1)));
    set(
        &rig.settings,
        r#"{"collection":{"enabled":false,"screen":true}}"#,
    );
    rig.screen.tick();
    assert!(!rig.dirs.screen_dir().exists());
    let row = rig.screen.status();
    assert_eq!(row["state"], "off");
    assert_eq!(row["reason"], "all collection is off");
}

#[test]
fn with_it_on_a_sample_is_stored_for_each_five_seconds_of_the_clock() {
    let rig = rig(Ok(reading(0)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    for n in 0..12 {
        // The person types between the samples, so no two are alike.
        rig.signals.answer(Ok(reading(n % 2)));
        rig.screen.tick();
        rig.clock.advance(Duration::from_secs(5));
    }
    let lines = lines(&rig.dirs.screen_dir());
    assert_eq!(lines.len(), 12);
    assert_eq!(samples(&lines), 12);
    let times: Vec<&str> = lines.iter().map(|l| l["time"].as_str().unwrap()).collect();
    let parsed: Vec<time::OffsetDateTime> = times
        .iter()
        .map(|t| {
            time::OffsetDateTime::parse(t, &time::format_description::well_known::Rfc3339).unwrap()
        })
        .collect();
    for pair in parsed.windows(2) {
        assert_eq!(pair[1] - pair[0], time::Duration::seconds(5));
    }
    let row = rig.screen.status();
    assert_eq!(row["state"], "collecting");
    assert_eq!(row["samples_today"], 12);
    assert_eq!(row["last_sample"], lines[11]["until"]);
    // The line holds the signals of the sample.
    assert_eq!(lines[0]["app"]["bundle_id"], "org.example.browser");
    assert_eq!(lines[0]["tab"]["private"], true);
    assert_eq!(lines[0]["call"], false);
    assert_eq!(lines[0]["locked"], false);
}

#[test]
fn the_files_are_readable_by_the_person_only() {
    use std::os::unix::fs::PermissionsExt;
    let rig = rig(Ok(reading(0)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    rig.screen.tick();
    let dir = rig.dirs.screen_dir();
    assert_eq!(
        fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    for entry in fs::read_dir(&dir).unwrap() {
        let mode = entry.unwrap().metadata().unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn equal_samples_make_one_line() {
    let rig = rig(Ok(reading(30)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    for n in 0..6 {
        // Nothing is touched: the idle time grows as the clock does.
        rig.signals.answer(Ok(reading(30 + 5 * n)));
        rig.screen.tick();
        rig.clock.advance(Duration::from_secs(5));
    }
    let after_the_run = lines(&rig.dirs.screen_dir());
    assert_eq!(after_the_run.len(), 1);
    assert_eq!(after_the_run[0]["samples"], 6);
    assert_ne!(after_the_run[0]["time"], after_the_run[0]["until"]);
    // A different window starts a line of its own.
    let mut other = reading(60);
    other.window_title = Some("Another window".into());
    rig.signals.answer(Ok(other));
    rig.screen.tick();
    let lines = lines(&rig.dirs.screen_dir());
    assert_eq!(lines.len(), 2);
    assert_eq!(samples(&lines), 7);
    assert_eq!(rig.screen.status()["samples_today"], 7);
}

#[test]
fn a_call_that_hangs_does_not_stop_the_next_sample() {
    let rig = rig(Ok(reading(1)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    let (release, hold) = channel();
    rig.signals.hang_next(hold);
    let started = std::time::Instant::now();
    rig.screen.tick();
    // Given up at the limit, not waited for until the call ends.
    assert!(started.elapsed() < CALL_LIMIT * 3);
    let row = rig.screen.status();
    assert_eq!(row["state"], "failed");
    assert_eq!(row["reason"], "the signals did not answer in time");
    assert!(lines(&rig.dirs.screen_dir()).is_empty());

    rig.clock.advance(Duration::from_secs(5));
    rig.screen.tick();
    assert_eq!(samples(&lines(&rig.dirs.screen_dir())), 1);
    assert_eq!(rig.screen.status()["state"], "collecting");
    // Let the thread of the first call end.
    release.send(()).unwrap();
}

#[test]
fn calls_that_never_return_are_not_piled_up() {
    let rig = rig(Ok(reading(1)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    let mut releases = Vec::new();
    for _ in 0..6 {
        let (release, hold) = channel();
        rig.signals.hang_next(hold);
        releases.push(release);
    }
    for _ in 0..6 {
        rig.screen.tick();
    }
    // The first calls hang, and no further call is made while the most allowed are out.
    assert_eq!(
        rig.signals.reads.load(Ordering::SeqCst),
        construct_agent::screen::MAX_STUCK
    );
    for release in &releases {
        let _ = release.send(());
    }
    // Once they return, the sampler asks again.
    let mut stored = false;
    for _ in 0..100 {
        rig.screen.tick();
        if samples(&lines(&rig.dirs.screen_dir())) > 0 {
            stored = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(stored);
}

#[test]
fn the_erase_removes_the_records() {
    let rig = rig(Ok(reading(1)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    rig.screen.tick();
    assert!(rig.dirs.screen_dir().exists());
    // Another source in the same folder is not touched.
    let other = rig.dirs.data().join("records").join("chat");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("a.jsonl"), "{}\n").unwrap();

    let stores = OwnStores::new(vec![Arc::new(rig.screen.clone()) as Arc<dyn OwnStore>]);
    assert!(stores.erase().is_empty());
    assert!(!rig.dirs.screen_dir().exists());
    assert!(other.join("a.jsonl").exists());
    let row = rig.screen.status();
    assert_eq!(row["samples_today"], 0);
    assert!(row.get("last_sample").is_none());

    // The next sample, one that would have continued the run, starts a new file: the line is
    // whole and holds the one sample.
    rig.clock.advance(Duration::from_secs(5));
    rig.signals.answer(Ok(reading(6)));
    rig.screen.tick();
    let after = lines(&rig.dirs.screen_dir());
    assert_eq!(after.len(), 1);
    assert_eq!(after[0]["samples"], 1);
}

#[test]
fn the_row_carries_the_grants_of_the_fake() {
    let rig = rig(Err(Failure::NeedsPermission));
    *rig.signals.grants.lock().unwrap() = vec![
        Grant {
            name: "accessibility",
            state: GrantState::Granted,
        },
        Grant {
            name: "screen_recording",
            state: GrantState::Denied,
        },
        Grant {
            name: "automation",
            state: GrantState::NotAsked,
        },
    ];
    // Off, the row still says what the person can grant.
    rig.screen.tick();
    assert_eq!(
        rig.screen.status()["grants"],
        json!([
            { "name": "accessibility", "state": "granted" },
            { "name": "screen_recording", "state": "denied" },
            { "name": "automation", "state": "not_asked" },
        ])
    );
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    rig.screen.tick();
    let row = rig.screen.status();
    assert_eq!(row["state"], "needs_permission");
    assert_eq!(row["reason"], "a permission is missing");
    assert_eq!(row["grants"][1]["state"], "denied");
    assert!(!rig.dirs.screen_dir().exists());
}

#[test]
fn a_failure_is_shown_as_its_code() {
    let rig = rig(Err(Failure::Failed("window_list")));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    rig.screen.tick();
    let row = rig.screen.status();
    assert_eq!(row["state"], "failed");
    assert_eq!(row["reason"], "window_list");
}

#[test]
fn a_computer_without_the_signals_reads_as_off() {
    let dirs = Dirs::new();
    let settings = dirs.settings();
    set(&settings, r#"{"collection":{"screen":true}}"#);
    let screen = Screen::new(
        settings,
        &dirs.data(),
        Arc::new(construct_agent::screen::NoSignals),
    );
    screen.tick();
    let row = screen.status();
    assert_eq!(row["state"], "off");
    assert_eq!(row["reason"], "not supported on this computer");
    assert!(!dirs.screen_dir().exists());
}

#[test]
fn samples_of_the_day_survive_a_restart() {
    let rig = rig(Ok(reading(0)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    for n in 0..3 {
        rig.signals.answer(Ok(reading(n % 2)));
        rig.screen.tick();
        rig.clock.advance(Duration::from_secs(5));
    }
    let again = Screen::with_timing(
        rig.settings.clone(),
        &rig.dirs.data(),
        rig.signals.clone(),
        rig.clock.clone(),
        Duration::from_secs(5),
        CALL_LIMIT,
    );
    assert_eq!(again.status()["samples_today"], 3);
    again.tick();
    assert_eq!(again.status()["samples_today"], 4);
    assert_eq!(samples(&lines(&rig.dirs.screen_dir())), 4);
}

#[test]
fn the_row_comes_over_the_socket_with_no_title_or_address() {
    let rig = rig(Ok(reading(1)));
    let path = rig
        .dirs
        .root
        .join("run")
        .join("construct")
        .join("agent.sock");
    let server = Server::bind(&path)
        .unwrap()
        .with_settings(rig.settings.clone())
        .with_screen(rig.screen.clone());
    thread::spawn(move || server.serve());

    let status: Value = serde_json::from_str(&query_status(&path).unwrap()).unwrap();
    assert_eq!(status["screen"]["state"], "off");

    let answer = request(
        &path,
        r#"settings-set {"collection":{"screen":true}}"#,
        Duration::from_secs(5),
    )
    .unwrap();
    let answer: Value = serde_json::from_str(&answer).unwrap();
    assert_eq!(answer["collection"]["screen"], true);
    assert_eq!(answer["supports"]["screen"], true);

    rig.screen.tick();
    let line = query_status(&path).unwrap();
    assert!(!line.contains(TITLE) && !line.contains(ADDRESS), "{line}");
    let status: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(status["screen"]["state"], "collecting");
    assert_eq!(status["screen"]["samples_today"], 1);

    let answer = request(&path, "settings-get", Duration::from_secs(5)).unwrap();
    assert!(!answer.contains(TITLE));
}

#[test]
fn a_reading_shows_no_text_in_debug() {
    let shown = format!("{:?}", reading(3));
    assert!(!shown.contains(TITLE) && !shown.contains(ADDRESS) && !shown.contains("Browser"));
}

#[test]
fn the_readers_of_session_logs_skip_the_samples() {
    let rig = rig(Ok(reading(1)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    rig.screen.tick();
    let sink = construct_agent::records::FileSink::new(&rig.dirs.data());
    assert!(sink.session_files().unwrap().is_empty());
}

/// The readings that differ from `reading(30)` in one thing each, and are not a run with it.
fn changed_readings() -> Vec<(&'static str, Reading)> {
    let mut app = reading(35);
    app.app = Some(App {
        name: "Browser".into(),
        bundle_id: "org.example.other".into(),
    });
    let mut url = reading(35);
    url.tab.as_mut().unwrap().url = "https://example.org/other".into();
    let mut private = reading(35);
    private.tab.as_mut().unwrap().private = false;
    let mut call = reading(35);
    call.call = true;
    let mut locked = reading(35);
    locked.locked = true;
    let mut window = reading(35);
    window.window_title = Some("Another window".into());
    let mut tab_title = reading(35);
    tab_title.tab.as_mut().unwrap().title = "Another tab".into();
    let mut input = reading(0);
    input.idle_seconds = 2;
    vec![
        ("window title", window),
        ("tab title", tab_title),
        ("app", app),
        ("address", url),
        ("private flag", private),
        ("call", call),
        ("lock", locked),
        ("input", input),
    ]
}

#[test]
fn a_change_of_any_one_thing_starts_a_new_line() {
    for (what, changed) in changed_readings() {
        let rig = rig(Ok(reading(30)));
        set(&rig.settings, r#"{"collection":{"screen":true}}"#);
        rig.screen.tick();
        rig.clock.advance(Duration::from_secs(5));
        rig.signals.answer(Ok(changed));
        rig.screen.tick();
        assert_eq!(lines(&rig.dirs.screen_dir()).len(), 2, "{what}");
    }
}

#[test]
fn a_run_does_not_span_a_time_nothing_was_stored() {
    let rig = rig(Ok(reading(30)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    rig.screen.tick();
    set(&rig.settings, r#"{"collection":{"screen":false}}"#);
    rig.clock.advance(Duration::from_secs(5));
    rig.screen.tick();
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    // An idle machine: the same things in front, and the idle time grew as the clock did.
    rig.clock.advance(Duration::from_secs(5));
    rig.signals.answer(Ok(reading(40)));
    rig.screen.tick();
    let lines = lines(&rig.dirs.screen_dir());
    assert_eq!(lines.len(), 2);
    assert_eq!(samples(&lines), 2);
}

#[test]
fn a_failed_step_ends_the_run() {
    let rig = rig(Ok(reading(30)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    rig.screen.tick();
    rig.clock.advance(Duration::from_secs(5));
    rig.signals.answer(Err(Failure::Failed("window_list")));
    rig.screen.tick();
    rig.clock.advance(Duration::from_secs(5));
    rig.signals.answer(Ok(reading(40)));
    rig.screen.tick();
    assert_eq!(lines(&rig.dirs.screen_dir()).len(), 2);
}

#[test]
fn nothing_is_stored_when_the_switch_goes_off_during_the_read() {
    let rig = rig(Ok(reading(1)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    let settings = rig.settings.clone();
    *rig.signals.during_read.lock().unwrap() = Some(Box::new(move || {
        set(&settings, r#"{"collection":{"screen":false}}"#);
    }));
    rig.screen.tick();
    assert!(!rig.dirs.screen_dir().exists());
    assert_eq!(rig.screen.status()["state"], "off");
}

#[test]
fn a_read_that_panics_is_named_and_does_not_end_the_sampling() {
    let rig = rig(Ok(reading(1)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    // More panics than slots: a slot lost for each would stop the sampling.
    for _ in 0..=construct_agent::screen::MAX_STUCK {
        rig.signals.panic_next.store(true, Ordering::SeqCst);
        rig.screen.tick();
        let row = rig.screen.status();
        assert_eq!(row["state"], "failed");
        assert_eq!(row["reason"], "the signals stopped unexpectedly");
    }
    rig.screen.tick();
    assert_eq!(samples(&lines(&rig.dirs.screen_dir())), 1);
}

#[test]
fn the_grants_are_looked_at_when_off_only_now_and_then() {
    let rig = rig(Ok(reading(1)));
    rig.screen.tick();
    assert_eq!(rig.screen.status()["grants"][0]["state"], "granted");
    // Off, the person's change of a grant shows after a while, not at the next step.
    *rig.signals.grants.lock().unwrap() = vec![Grant {
        name: "accessibility",
        state: GrantState::Denied,
    }];
    rig.screen.tick();
    assert_eq!(rig.screen.status()["grants"][0]["state"], "granted");
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    rig.screen.tick();
    assert_eq!(rig.screen.status()["grants"][0]["state"], "denied");
}

#[test]
fn the_first_sample_of_a_new_day_goes_to_a_new_file() {
    let rig = rig(Ok(reading(1)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    rig.screen.tick();
    assert_eq!(rig.screen.status()["samples_today"], 1);
    // A day and a half later is another local day in every time zone.
    rig.clock.advance(Duration::from_hours(36));
    assert_eq!(rig.screen.status()["samples_today"], 0);
    assert!(rig.screen.status().get("last_sample").is_none());
    rig.signals.answer(Ok(reading(2)));
    rig.screen.tick();
    assert_eq!(fs::read_dir(rig.dirs.screen_dir()).unwrap().count(), 2);
    assert_eq!(rig.screen.status()["samples_today"], 1);
}

#[test]
fn a_torn_last_line_costs_that_line_only() {
    let rig = rig(Ok(reading(1)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    rig.screen.tick();
    let file = fs::read_dir(rig.dirs.screen_dir())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    // A crash left half a line, with a byte that is no text.
    let mut torn = fs::read(&file).unwrap();
    torn.extend_from_slice(b"{\"time\":\"2026-11-1\xFF");
    fs::write(&file, &torn).unwrap();
    // The samples before it still count after a restart.
    let again = Screen::with_timing(
        rig.settings.clone(),
        &rig.dirs.data(),
        rig.signals.clone(),
        rig.clock.clone(),
        Duration::from_secs(5),
        CALL_LIMIT,
    );
    assert_eq!(again.status()["samples_today"], 1);
    // The next line starts on a line of its own, and the run that follows continues on it.
    rig.signals.answer(Ok(reading(2)));
    again.tick();
    rig.clock.advance(Duration::from_secs(5));
    rig.signals.answer(Ok(reading(7)));
    again.tick();
    let text = String::from_utf8_lossy(&fs::read(&file).unwrap()).into_owned();
    let good: Vec<Value> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect();
    assert_eq!(good.len(), 2);
    assert_eq!(good[1]["samples"], 2);
    assert_eq!(again.status()["samples_today"], 3);
}

#[test]
fn the_sampler_takes_a_sample_each_interval_and_stops_when_dropped() {
    let rig = rig(Ok(reading(1)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    let screen = Screen::with_timing(
        rig.settings.clone(),
        &rig.dirs.data(),
        rig.signals.clone(),
        rig.clock.clone(),
        Duration::from_millis(20),
        CALL_LIMIT,
    );
    let alive = screen.spawn();
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while rig.signals.reads.load(Ordering::SeqCst) < 4 {
        assert!(
            std::time::Instant::now() < deadline,
            "no sample was taken again"
        );
        thread::sleep(Duration::from_millis(10));
    }
    drop(alive);
    // A step that was running may end; then nothing more is read.
    thread::sleep(Duration::from_millis(300));
    let stopped_at = rig.signals.reads.load(Ordering::SeqCst);
    thread::sleep(Duration::from_millis(300));
    assert_eq!(rig.signals.reads.load(Ordering::SeqCst), stopped_at);
    assert!(samples(&lines(&rig.dirs.screen_dir())) >= 4);
}

#[test]
fn the_list_of_the_agents_own_stores_reaches_the_screen_samples() {
    use construct_agent::activity::Feed;
    use construct_agent::chat::Link;
    use construct_agent::erase::own_stores;
    use construct_agent::follow::Erase;
    use construct_agent::manage::Manager;

    let rig = rig(Ok(reading(1)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    rig.screen.tick();
    assert!(rig.dirs.screen_dir().exists());

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let data = rig.dirs.data();
    let manage = Manager::open(
        &rig.dirs.root.join("manage"),
        rig.settings.clone(),
        Link::default(),
    )
    .unwrap();
    let sessions = Erase::new(&data, &["claude-code", "codex"]);
    let stores = own_stores(
        &sessions,
        None,
        &manage,
        &Feed::new(),
        None,
        Some(&rig.screen),
        runtime.handle(),
    );
    let failed = stores.erase();
    assert!(failed.is_empty(), "{failed:?}");
    assert!(!rig.dirs.screen_dir().exists());
}

#[test]
fn a_gap_longer_than_a_few_intervals_ends_the_run() {
    let rig = rig(Ok(reading(30)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    rig.screen.tick();
    // A suspended computer: the clock and the idle time both ran on, the window did not change.
    rig.clock.advance(Duration::from_secs(600));
    rig.signals.answer(Ok(reading(630)));
    rig.screen.tick();
    let lines = lines(&rig.dirs.screen_dir());
    assert_eq!(lines.len(), 2);
    assert_eq!(samples(&lines), 2);
}

#[test]
fn a_run_is_cut_after_an_hour() {
    let rig = rig(Ok(reading(0)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    let mut idle = 0;
    // 400 samples ten seconds apart on one local day: an hour is 361 of them, the rest are a
    // line of their own.
    for _ in 0..400 {
        rig.signals.answer(Ok(reading(idle)));
        rig.screen.tick();
        rig.clock.advance(Duration::from_secs(10));
        idle += 10;
    }
    let lines = lines(&rig.dirs.screen_dir());
    let counts: Vec<u64> = lines
        .iter()
        .map(|l| l["samples"].as_u64().unwrap())
        .collect();
    assert_eq!(counts, [361, 39]);
}

#[test]
fn a_file_that_is_not_as_the_run_left_it_gets_a_line_of_its_own() {
    // Removed from outside, or cut.
    for cut in [false, true] {
        let rig = rig(Ok(reading(30)));
        set(&rig.settings, r#"{"collection":{"screen":true}}"#);
        rig.screen.tick();
        if cut {
            let file = fs::read_dir(rig.dirs.screen_dir())
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            fs::write(file, "").unwrap();
        } else {
            fs::remove_dir_all(rig.dirs.screen_dir()).unwrap();
        }
        rig.clock.advance(Duration::from_secs(5));
        rig.signals.answer(Ok(reading(35)));
        rig.screen.tick();
        assert_eq!(rig.screen.status()["state"], "collecting", "cut: {cut}");
        let lines = lines(&rig.dirs.screen_dir());
        assert_eq!(lines.len(), 1, "cut: {cut}");
        assert_eq!(lines[0]["samples"], 1, "cut: {cut}");
    }
}

#[test]
fn a_run_ended_by_a_switch_off_during_the_read_is_not_continued() {
    let rig = rig(Ok(reading(30)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    rig.screen.tick();
    let settings = rig.settings.clone();
    *rig.signals.during_read.lock().unwrap() = Some(Box::new(move || {
        set(&settings, r#"{"collection":{"screen":false}}"#);
    }));
    rig.clock.advance(Duration::from_secs(5));
    rig.signals.answer(Ok(reading(35)));
    rig.screen.tick();
    *rig.signals.during_read.lock().unwrap() = None;
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    rig.clock.advance(Duration::from_secs(5));
    rig.signals.answer(Ok(reading(40)));
    rig.screen.tick();
    assert_eq!(lines(&rig.dirs.screen_dir()).len(), 2);
}

#[test]
fn after_a_failed_write_the_row_still_counts_what_the_file_holds() {
    use std::os::unix::fs::PermissionsExt;
    let rig = rig(Ok(reading(0)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    for n in 0..3 {
        rig.signals.answer(Ok(reading(n % 2)));
        rig.screen.tick();
        rig.clock.advance(Duration::from_secs(5));
    }
    let file = fs::read_dir(rig.dirs.screen_dir())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    // A new agent, and a file it cannot write.
    let again = Screen::with_timing(
        rig.settings.clone(),
        &rig.dirs.data(),
        rig.signals.clone(),
        rig.clock.clone(),
        Duration::from_secs(5),
        CALL_LIMIT,
    );
    fs::set_permissions(&file, fs::Permissions::from_mode(0o400)).unwrap();
    rig.signals.answer(Ok(reading(1)));
    again.tick();
    let row = again.status();
    assert_eq!(row["state"], "failed");
    assert_eq!(row["reason"], "the samples could not be written");
    assert_eq!(row["samples_today"], 3);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    rig.clock.advance(Duration::from_secs(5));
    rig.signals.answer(Ok(reading(0)));
    again.tick();
    assert_eq!(again.status()["samples_today"], 4);
}

#[test]
fn the_grants_are_looked_at_again_on_the_twelfth_step_while_off() {
    let rig = rig(Ok(reading(1)));
    rig.screen.tick();
    *rig.signals.grants.lock().unwrap() = vec![Grant {
        name: "accessibility",
        state: GrantState::Denied,
    }];
    for _ in 1..12 {
        rig.screen.tick();
        assert_eq!(rig.screen.status()["grants"][0]["state"], "granted");
    }
    rig.screen.tick();
    assert_eq!(rig.screen.status()["grants"][0]["state"], "denied");
}

#[test]
fn a_grants_call_that_hangs_drops_the_grants_and_does_not_stop_the_samples() {
    let rig = rig(Ok(reading(1)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    rig.screen.tick();
    assert_eq!(rig.screen.status()["grants"][0]["state"], "granted");
    let (release, hold) = channel();
    rig.signals.hang_grants.lock().unwrap().push(hold);
    rig.clock.advance(Duration::from_secs(5));
    rig.signals.answer(Ok(reading(9)));
    rig.screen.tick();
    let row = rig.screen.status();
    assert_eq!(row["grants"], json!([]));
    assert_eq!(row["state"], "collecting");
    assert_eq!(samples(&lines(&rig.dirs.screen_dir())), 2);
    release.send(()).unwrap();
}

#[test]
fn an_idle_machine_over_local_midnight_starts_a_line_in_the_new_day() {
    let rig = rig(Ok(reading(1)));
    set(&rig.settings, r#"{"collection":{"screen":true}}"#);
    // Five seconds apart, the last two after midnight, nothing touched: the idle time grows as
    // the clock does, so only the day tells the lines apart.
    rig.clock
        .set(next_local_midnight(rig.clock.now()) - Duration::from_secs(12));
    let mut idle = 1;
    for _ in 0..6 {
        rig.signals.answer(Ok(reading(idle)));
        rig.screen.tick();
        rig.clock.advance(Duration::from_secs(5));
        idle += 5;
    }
    assert_eq!(fs::read_dir(rig.dirs.screen_dir()).unwrap().count(), 2);
    let counts: Vec<u64> = lines(&rig.dirs.screen_dir())
        .iter()
        .map(|l| l["samples"].as_u64().unwrap())
        .collect();
    assert_eq!(counts, [3, 3]);
}

#[test]
fn a_run_stays_one_line_when_the_times_have_fractions_of_different_length() {
    let dirs = Dirs::new();
    let settings = dirs.settings();
    set(&settings, r#"{"collection":{"screen":true}}"#);
    let signals = FakeSignals::new(Ok(reading(30)));
    let base = local_noon();
    let clock = FakeClock::at(base + Duration::from_nanos(123_456_789));
    let screen = Screen::with_timing(
        settings,
        &dirs.data(),
        signals.clone(),
        clock.clone(),
        Duration::from_secs(5),
        CALL_LIMIT,
    );
    // The time of each sample is written with more or fewer digits than the one before it: a
    // whole second has no fraction at all.
    let offsets = [
        123_456_789,
        5_000_000_000,
        10_100_000_000,
        15_000_000_000,
        20_999_999_999,
    ];
    for (n, offset) in offsets.into_iter().enumerate() {
        clock.set(base + Duration::from_nanos(offset));
        signals.answer(Ok(reading(30 + 5 * n as u64)));
        screen.tick();
    }
    let file = fs::read_dir(dirs.screen_dir())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let text = fs::read_to_string(file).unwrap();
    // One whole line, nothing left behind it.
    assert_eq!(text.lines().count(), 1, "{text}");
    let line: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(line["samples"], 5);
    assert!(text.ends_with('\n'));
}

#[test]
fn the_settings_answer_follows_what_the_signals_support() {
    let dirs = Dirs::new();
    let settings = dirs.settings();
    let _screen = Screen::new(
        settings.clone(),
        &dirs.data(),
        Arc::new(construct_agent::screen::NoSignals),
    );
    let answer: Value = serde_json::from_str(&settings.get_response()).unwrap();
    assert_eq!(answer["supports"]["screen"], false);

    let rig = rig(Ok(reading(1)));
    let answer: Value = serde_json::from_str(&rig.settings.get_response()).unwrap();
    assert_eq!(answer["supports"]["screen"], true);
}
