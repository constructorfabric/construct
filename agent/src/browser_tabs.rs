//! The open tab of Chrome and Safari, asked through `AppleScript`.
//!
//! Each browser is asked with `osascript` and answers one short text: whether it runs, and for its
//! front window the title and address of the open tab (Chrome also says whether the window is a
//! private one; Safari does not say, so its windows count as not private). The text is parsed here,
//! on any system, so the parsing is tested everywhere; only the call itself needs a Mac.
//!
//! * A browser that is not running is never started: the script asks `is running` first, and the
//!   `tell` that would launch it is inside the answer's `then`.
//! * Every call has a time limit ([`CALL_LIMIT`], or what is left of the caller's deadline); a call
//!   that does not answer in time is killed and counts as no answer. An empty answer is asked
//!   once more while there is time. An answer longer than [`MAX_OUTPUT`] is no answer.
//! * A bundle id that no application has (a browser that is not installed) is said by error -1728
//!   and counts as not running.
//! * The system refuses with error -1743 when the person has not allowed the agent to control the
//!   browser; that is a missing grant, said as such.
//!
//! A title or an address is never logged or put in an error: nothing here prints them, and the
//! errors are fixed words.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::screen::Tab;

/// The longest one call to a browser may take.
pub const CALL_LIMIT: Duration = Duration::from_millis(400);
/// A call is not started with less time than this left.
const MIN_CALL: Duration = Duration::from_millis(50);
/// How long the pipes are waited for once the program has ended.
const PIPE_WAIT: Duration = Duration::from_millis(100);
/// The most a program may print. A longer answer is not stored cut: it is no answer.
const MAX_OUTPUT: u64 = 32 * 1024;
/// Separates the fields of an answer (never in a title or an address).
const FIELD: char = '\u{1f}';
/// What the script answers when the browser is not running.
const NOT_RUNNING: &str = "not-running";
/// What the script answers when the browser has no window.
const NO_WINDOW: &str = "no-window";
/// What the script answers for a window that was read.
const WINDOW: &str = "window";
/// The system's error number for "not allowed to send Apple events".
const NOT_ALLOWED_CODE: &str = "-1743";
/// The system's error number for "no application has this id", which is what a browser that is
/// not installed gives.
const NOT_INSTALLED_CODE: &str = "-1728";

/// A browser whose tab can be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Browser {
    Chrome,
    Safari,
}

impl Browser {
    pub const ALL: [Browser; 2] = [Browser::Chrome, Browser::Safari];

    /// The browser whose bundle id this is.
    #[must_use]
    pub fn from_bundle_id(bundle_id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|b| b.bundle_id().eq_ignore_ascii_case(bundle_id))
    }

    #[must_use]
    pub fn bundle_id(self) -> &'static str {
        match self {
            Browser::Chrome => "com.google.Chrome",
            Browser::Safari => "com.apple.Safari",
        }
    }

    /// The name the person knows it by.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Browser::Chrome => "Chrome",
            Browser::Safari => "Safari",
        }
    }

    /// The script that reads the open tab of the front window.
    fn tab_script(self) -> String {
        let id = self.bundle_id();
        // Chrome says the mode of a window ("normal" or "incognito"); Safari has no such property.
        let (mode, title, url) = match self {
            Browser::Chrome => (
                "mode of w",
                "title of active tab of w",
                "URL of active tab of w",
            ),
            Browser::Safari => (
                "\"normal\"",
                "name of current tab of w",
                "URL of current tab of w",
            ),
        };
        format!(
            r#"if application id "{id}" is not running then return "{NOT_RUNNING}"
tell application id "{id}"
    if (count of windows) is 0 then return "{NO_WINDOW}"
    set w to front window
    set sep to (character id 31)
    return "{WINDOW}" & sep & ({mode}) & sep & ({title}) & sep & ({url})
end tell"#
        )
    }

    /// The script that only touches the browser, to learn whether it may be asked.
    fn probe_script(self) -> String {
        let id = self.bundle_id();
        format!(
            r#"if application id "{id}" is not running then return "{NOT_RUNNING}"
tell application id "{id}" to return count of windows"#
        )
    }
}

/// What a call to `osascript` gave.
#[derive(PartialEq, Eq)]
pub enum Answer {
    /// What the script printed, without the closing line break.
    Text(String),
    /// The person has not allowed the agent to control the browser.
    NotAllowed,
    /// No application has the browser's id: it is not installed.
    NotInstalled,
    /// No answer in time, a failure to run, or any other error.
    Unavailable,
}

// The text is a title and an address: it is never printed, so a failed assertion cannot leak one.
impl std::fmt::Debug for Answer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Answer::Text(_) => "Text",
            Answer::NotAllowed => "NotAllowed",
            Answer::NotInstalled => "NotInstalled",
            Answer::Unavailable => "Unavailable",
        })
    }
}

/// Runs a script, giving it up after `limit`. The system's program is [`Osascript`]; the tests give
/// their own.
pub trait ScriptRunner: Send + Sync {
    fn run(&self, script: &str, limit: Duration) -> Answer;
}

/// Runs scripts with `osascript`, killing a call that takes longer than its limit.
///
/// The call is made here and not with the chat tool's runner because that one is asynchronous and
/// meant for the model's commands; the signals are read on a thread of their own and need a plain
/// blocking call.
pub struct Osascript;

impl ScriptRunner for Osascript {
    fn run(&self, script: &str, limit: Duration) -> Answer {
        run_osascript(script, limit)
    }
}

/// The time one call may have: [`CALL_LIMIT`], or what is left before `deadline` when that is
/// less. `None` when too little is left to ask.
fn budget(deadline: Instant) -> Option<Duration> {
    let left = deadline.saturating_duration_since(Instant::now());
    (left >= MIN_CALL).then(|| left.min(CALL_LIMIT))
}

fn run_osascript(script: &str, limit: Duration) -> Answer {
    run_limited("osascript", &["-e", script], limit)
}

/// Runs `program` and gives up on it, killing it, after `limit`.
fn run_limited(program: &str, args: &[&str], limit: Duration) -> Answer {
    let Ok(mut child) = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return Answer::Unavailable;
    };
    let (tx, rx) = mpsc::channel();
    for (index, pipe) in [
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .enumerate()
    {
        let tx = tx.clone();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(pipe) = pipe {
                // One byte more than the most, to tell a full answer from a longer one.
                let _ = pipe.take(MAX_OUTPUT + 1).read_to_end(&mut bytes);
            }
            let _ = tx.send((index, bytes));
        });
    }
    drop(tx);
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                // A hung browser: the call is given up, and the program with it.
                let _ = child.kill();
                let _ = child.wait();
                return Answer::Unavailable;
            }
        }
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    for _ in 0..2 {
        match rx.recv_timeout(PIPE_WAIT) {
            Ok((0, bytes)) => out = bytes,
            Ok((_, bytes)) => err = bytes,
            Err(_) => return Answer::Unavailable,
        }
    }
    if out.len() as u64 > MAX_OUTPUT {
        // The program may still be writing into a pipe nobody reads.
        let _ = child.kill();
        let _ = child.wait();
        return Answer::Unavailable;
    }
    let err = String::from_utf8_lossy(&err);
    if status.success() {
        Answer::Text(trim_line_break(&String::from_utf8_lossy(&out)).to_owned())
    } else if err.contains(NOT_ALLOWED_CODE) {
        Answer::NotAllowed
    } else if err.contains(NOT_INSTALLED_CODE) {
        Answer::NotInstalled
    } else {
        Answer::Unavailable
    }
}

fn trim_line_break(text: &str) -> &str {
    text.strip_suffix('\n').unwrap_or(text)
}

/// What asking a browser for its open tab gave.
#[derive(PartialEq, Eq)]
pub enum TabRead {
    Tab(Tab),
    /// The browser runs and answered, with no window or no readable tab.
    NoTab,
    NotRunning,
    NotAllowed,
    Unavailable,
}

// Never prints a title or an address, so a failed assertion cannot leak one either.
impl std::fmt::Debug for TabRead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            TabRead::Tab(_) => "Tab",
            TabRead::NoTab => "NoTab",
            TabRead::NotRunning => "NotRunning",
            TabRead::NotAllowed => "NotAllowed",
            TabRead::Unavailable => "Unavailable",
        })
    }
}

/// The open tab of the front window of `browser`, asked before `deadline`. An empty answer is
/// asked once more, when there is time left.
#[must_use]
pub fn read_tab(runner: &dyn ScriptRunner, browser: Browser, deadline: Instant) -> TabRead {
    let script = browser.tab_script();
    for _ in 0..2 {
        let Some(limit) = budget(deadline) else {
            return TabRead::Unavailable;
        };
        match runner.run(&script, limit) {
            Answer::Text(text) if text.is_empty() => {}
            Answer::Text(text) => return parse_tab(&text),
            Answer::NotAllowed => return TabRead::NotAllowed,
            Answer::NotInstalled => return TabRead::NotRunning,
            Answer::Unavailable => return TabRead::Unavailable,
        }
    }
    TabRead::Unavailable
}

/// What the browser's grant is, learned by touching it. The very first touch is what makes the
/// system ask the person.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reach {
    Allowed,
    NotAllowed,
    /// Not running, or not installed: there is nothing to ask.
    NotRunning,
    #[default]
    Unknown,
}

/// Touches `browser` to learn its grant, before `deadline`.
#[must_use]
pub fn probe(runner: &dyn ScriptRunner, browser: Browser, deadline: Instant) -> Reach {
    let Some(limit) = budget(deadline) else {
        return Reach::Unknown;
    };
    match runner.run(&browser.probe_script(), limit) {
        Answer::Text(text) if text == NOT_RUNNING => Reach::NotRunning,
        Answer::Text(text) if !text.is_empty() => Reach::Allowed,
        Answer::NotAllowed => Reach::NotAllowed,
        Answer::NotInstalled => Reach::NotRunning,
        Answer::Text(_) | Answer::Unavailable => Reach::Unknown,
    }
}

/// The tab in an answer of [`Browser::tab_script`].
#[must_use]
pub fn parse_tab(text: &str) -> TabRead {
    match text {
        NOT_RUNNING => return TabRead::NotRunning,
        NO_WINDOW => return TabRead::NoTab,
        _ => {}
    }
    let fields: Vec<&str> = text.split(FIELD).collect();
    let [kind, mode, title, url] = fields.as_slice() else {
        return TabRead::NoTab;
    };
    // A tab with no address (a new tab page that has none yet) has nothing to count.
    if *kind != WINDOW || url.is_empty() {
        return TabRead::NoTab;
    }
    TabRead::Tab(Tab {
        title: (*title).to_owned(),
        url: (*url).to_owned(),
        private: *mode == "incognito",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Answers one after another, and counts the calls and keeps the limits they were given.
    struct Fake(Mutex<Vec<Answer>>, Mutex<Vec<Duration>>);

    impl Fake {
        fn new(mut answers: Vec<Answer>) -> Self {
            answers.reverse();
            Fake(Mutex::new(answers), Mutex::new(Vec::new()))
        }
        fn calls(&self) -> usize {
            self.1.lock().unwrap().len()
        }
    }

    impl ScriptRunner for Fake {
        fn run(&self, _script: &str, limit: Duration) -> Answer {
            self.1.lock().unwrap().push(limit);
            self.0.lock().unwrap().pop().unwrap_or(Answer::Unavailable)
        }
    }

    /// A time far enough off that no budget is short.
    fn later() -> Instant {
        Instant::now() + Duration::from_secs(60)
    }

    fn window(mode: &str, title: &str, url: &str) -> String {
        format!("window\u{1f}{mode}\u{1f}{title}\u{1f}{url}")
    }

    #[test]
    fn a_chrome_answer_is_a_tab() {
        let text = window("normal", "Pull requests", "https://example.org/pulls");
        assert_eq!(
            parse_tab(&text),
            TabRead::Tab(Tab {
                title: "Pull requests".into(),
                url: "https://example.org/pulls".into(),
                private: false,
            })
        );
    }

    #[test]
    fn a_chrome_incognito_window_is_private() {
        let text = window("incognito", "Secret", "https://example.org/s");
        let TabRead::Tab(tab) = parse_tab(&text) else {
            panic!("a tab");
        };
        assert!(tab.private);
    }

    #[test]
    fn a_safari_answer_is_a_tab_that_is_not_private() {
        let text = window("normal", "Docs", "https://example.org/docs");
        let TabRead::Tab(tab) = parse_tab(&text) else {
            panic!("a tab");
        };
        assert_eq!((tab.title.as_str(), tab.private), ("Docs", false));
    }

    #[test]
    fn a_title_with_commas_quotes_and_line_breaks_is_kept_whole() {
        let text = window("normal", "a, \"b\"\nc", "https://example.org/?q=1&r=2");
        let TabRead::Tab(tab) = parse_tab(&text) else {
            panic!("a tab");
        };
        assert_eq!(tab.title, "a, \"b\"\nc");
        assert_eq!(tab.url, "https://example.org/?q=1&r=2");
    }

    #[test]
    fn the_fixed_answers_and_odd_ones_are_read_as_what_they_say() {
        assert_eq!(parse_tab("not-running"), TabRead::NotRunning);
        assert_eq!(parse_tab("no-window"), TabRead::NoTab);
        // A tab with no address, a short answer and a stranger's text are no tab.
        assert_eq!(parse_tab(&window("normal", "New Tab", "")), TabRead::NoTab);
        assert_eq!(parse_tab("window\u{1f}normal"), TabRead::NoTab);
        assert_eq!(parse_tab("something else"), TabRead::NoTab);
        assert_eq!(
            parse_tab(&format!("{}\u{1f}x", window("a", "b", "c"))),
            TabRead::NoTab
        );
    }

    #[test]
    fn a_browser_is_found_by_its_bundle_id() {
        assert_eq!(
            Browser::from_bundle_id("com.google.Chrome"),
            Some(Browser::Chrome)
        );
        assert_eq!(
            Browser::from_bundle_id("com.apple.safari"),
            Some(Browser::Safari)
        );
        assert_eq!(Browser::from_bundle_id("org.mozilla.firefox"), None);
        assert_eq!(Browser::from_bundle_id("com.google.Chrome.canary"), None);
    }

    #[test]
    fn every_script_opens_with_the_running_check_so_no_browser_is_started() {
        for browser in Browser::ALL {
            let guard = format!(
                "if application id \"{}\" is not running then return \"not-running\"\n",
                browser.bundle_id()
            );
            for script in [browser.tab_script(), browser.probe_script()] {
                assert!(script.starts_with(&guard), "{script}");
                // The one `tell` comes after it, so nothing talks to the browser before.
                assert_eq!(script.matches("tell application").count(), 1);
            }
        }
    }

    #[test]
    fn each_browser_asks_for_its_own_properties() {
        let chrome = Browser::Chrome.tab_script();
        assert!(chrome.contains("(mode of w)"));
        assert!(chrome.contains("title of active tab of w"));
        let safari = Browser::Safari.tab_script();
        assert!(safari.contains("(\"normal\")"));
        assert!(safari.contains("name of current tab of w"));
        assert!(!safari.contains("mode of"));
    }

    #[test]
    fn an_empty_answer_is_asked_once_more() {
        let good = window("normal", "T", "https://example.org/");
        let runner = Fake::new(vec![Answer::Text(String::new()), Answer::Text(good)]);
        assert!(matches!(
            read_tab(&runner, Browser::Chrome, later()),
            TabRead::Tab(_)
        ));
        assert_eq!(runner.calls(), 2);

        let runner = Fake::new(vec![
            Answer::Text(String::new()),
            Answer::Text(String::new()),
        ]);
        assert_eq!(
            read_tab(&runner, Browser::Chrome, later()),
            TabRead::Unavailable
        );
        assert_eq!(runner.calls(), 2);
    }

    #[test]
    fn a_missing_grant_or_no_answer_is_not_asked_again() {
        let runner = Fake::new(vec![Answer::NotAllowed]);
        assert_eq!(
            read_tab(&runner, Browser::Safari, later()),
            TabRead::NotAllowed
        );
        assert_eq!(runner.calls(), 1);
        let runner = Fake::new(vec![Answer::Unavailable]);
        assert_eq!(
            read_tab(&runner, Browser::Safari, later()),
            TabRead::Unavailable
        );
        assert_eq!(runner.calls(), 1);
    }

    #[test]
    fn a_probe_tells_the_grant_apart() {
        let probe_of = |answer| probe(&Fake::new(vec![answer]), Browser::Chrome, later());
        assert_eq!(probe_of(Answer::Text("2".into())), Reach::Allowed);
        assert_eq!(
            probe_of(Answer::Text("not-running".into())),
            Reach::NotRunning
        );
        assert_eq!(probe_of(Answer::NotAllowed), Reach::NotAllowed);
        assert_eq!(probe_of(Answer::Unavailable), Reach::Unknown);
        assert_eq!(probe_of(Answer::Text(String::new())), Reach::Unknown);
    }

    #[cfg(unix)]
    #[test]
    fn a_hung_call_is_given_up_at_the_limit() {
        // `osascript` is not here (or not a Mac), so any program stands in: the limit is the
        // runner's own, applied to a program that does not end.
        let started = Instant::now();
        let answer = run_limited("sleep", &["30"], Duration::from_millis(150));
        assert_eq!(answer, Answer::Unavailable);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_answers_gives_its_text_and_a_refusal_its_code() {
        let ok = run_limited("sh", &["-c", "printf 'hello\\n'"], Duration::from_secs(5));
        assert_eq!(ok, Answer::Text("hello".into()));
        let refused = run_limited(
            "sh",
            &["-c", "echo 'not allowed (-1743)' >&2; exit 1"],
            Duration::from_secs(5),
        );
        assert_eq!(refused, Answer::NotAllowed);
        let failed = run_limited(
            "sh",
            &["-c", "echo boom >&2; exit 1"],
            Duration::from_secs(5),
        );
        assert_eq!(failed, Answer::Unavailable);
    }

    #[test]
    fn a_browser_that_is_not_installed_is_not_running() {
        let runner = Fake::new(vec![Answer::NotInstalled]);
        assert_eq!(
            read_tab(&runner, Browser::Chrome, later()),
            TabRead::NotRunning
        );
        let probed = probe(
            &Fake::new(vec![Answer::NotInstalled]),
            Browser::Safari,
            later(),
        );
        assert_eq!(probed, Reach::NotRunning);
    }

    #[test]
    fn no_call_is_made_without_time_and_a_call_gets_no_more_than_is_left() {
        let runner = Fake::new(vec![Answer::Text("not-running".into())]);
        let over = Instant::now();
        assert_eq!(
            read_tab(&runner, Browser::Chrome, over),
            TabRead::Unavailable
        );
        assert_eq!(probe(&runner, Browser::Chrome, over), Reach::Unknown);
        assert_eq!(runner.calls(), 0);

        // With plenty of time a call has the usual limit; with less left, what is left.
        let _ = read_tab(&runner, Browser::Chrome, later());
        let short = Instant::now() + Duration::from_millis(200);
        let _ = probe(&runner, Browser::Chrome, short);
        let limits = runner.1.lock().unwrap().clone();
        assert_eq!(limits[0], CALL_LIMIT);
        assert!(limits[1] <= Duration::from_millis(200) && limits[1] >= MIN_CALL);
    }

    #[test]
    fn the_debug_of_an_answer_shows_no_text() {
        let answer = Answer::Text("https://example.org/secret".into());
        assert_eq!(format!("{answer:?}"), "Text");
    }

    #[cfg(unix)]
    #[test]
    fn a_hung_program_is_killed_and_not_left_running() {
        let dir = std::env::temp_dir().join(format!("browser-tabs-pid-{}", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let script = format!("echo $$ > {}; exec sleep 30", dir.display());
        let answer = run_limited("sh", &["-c", &script], Duration::from_millis(300));
        assert_eq!(answer, Answer::Unavailable);
        let pid = std::fs::read_to_string(&dir).unwrap();
        let _ = std::fs::remove_file(&dir);
        let alive = std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(!alive, "the program was killed");
    }

    #[cfg(unix)]
    #[test]
    fn an_answer_longer_than_the_most_is_no_answer() {
        let long = format!("head -c {} /dev/zero | tr '\\0' a", MAX_OUTPUT + 10);
        let answer = run_limited("sh", &["-c", &long], Duration::from_secs(5));
        assert_eq!(answer, Answer::Unavailable);
        let exact = format!("head -c {MAX_OUTPUT} /dev/zero | tr '\\0' a");
        let Answer::Text(text) = run_limited("sh", &["-c", &exact], Duration::from_secs(5)) else {
            panic!("an answer at the most is kept");
        };
        assert_eq!(text.len() as u64, MAX_OUTPUT);
    }

    #[cfg(unix)]
    #[test]
    fn the_system_says_a_browser_is_not_installed_by_its_error_number() {
        let missing = run_limited(
            "sh",
            &[
                "-c",
                "echo \"Can't get application id (-1728)\" >&2; exit 1",
            ],
            Duration::from_secs(5),
        );
        assert_eq!(missing, Answer::NotInstalled);
    }
}
