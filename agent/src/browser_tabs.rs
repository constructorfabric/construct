//! The open tab of Chrome and Safari, asked through `AppleScript`.
//!
//! Each browser is asked with `osascript` and answers one short text: whether it runs, and for its
//! front window the title and address of the open tab (Chrome also says whether the window is a
//! private one; Safari does not say, so its windows count as not private). The text is parsed here,
//! on any system, so the parsing is tested everywhere; only the call itself needs a Mac.
//!
//! * A browser that is not running is never started: the script asks `is running` first, and the
//!   `tell` that would launch it is inside the answer's `then`.
//! * Every call has a time limit ([`CALL_LIMIT`]); a call that does not answer in time is killed
//!   and counts as no answer. An empty answer is asked once more.
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
#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    /// What the script printed, without the closing line break.
    Text(String),
    /// The person has not allowed the agent to control the browser.
    NotAllowed,
    /// No answer in time, a failure to run, or any other error.
    Unavailable,
}

/// Runs a script. The system's program is [`Osascript`]; the tests give their own.
pub trait ScriptRunner: Send + Sync {
    fn run(&self, script: &str) -> Answer;
}

/// Runs scripts with `osascript`, killing a call that takes longer than [`CALL_LIMIT`].
pub struct Osascript;

impl ScriptRunner for Osascript {
    fn run(&self, script: &str) -> Answer {
        run_osascript(script, CALL_LIMIT)
    }
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
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
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
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok((0, bytes)) => out = bytes,
            Ok((_, bytes)) => err = bytes,
            Err(_) => return Answer::Unavailable,
        }
    }
    if status.success() {
        Answer::Text(trim_line_break(&String::from_utf8_lossy(&out)).to_owned())
    } else if String::from_utf8_lossy(&err).contains(NOT_ALLOWED_CODE) {
        Answer::NotAllowed
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

/// The open tab of the front window of `browser`. An empty answer is asked once more.
#[must_use]
pub fn read_tab(runner: &dyn ScriptRunner, browser: Browser) -> TabRead {
    let script = browser.tab_script();
    for _ in 0..2 {
        match runner.run(&script) {
            Answer::Text(text) if text.is_empty() => {}
            Answer::Text(text) => return parse_tab(&text),
            Answer::NotAllowed => return TabRead::NotAllowed,
            Answer::Unavailable => return TabRead::Unavailable,
        }
    }
    TabRead::Unavailable
}

/// What the browser's grant is, learned by touching it. Asks the person on the very first call,
/// which is how the system grants it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reach {
    Allowed,
    NotAllowed,
    NotRunning,
    #[default]
    Unknown,
}

#[must_use]
pub fn probe(runner: &dyn ScriptRunner, browser: Browser) -> Reach {
    match runner.run(&browser.probe_script()) {
        Answer::Text(text) if text == NOT_RUNNING => Reach::NotRunning,
        Answer::Text(text) if !text.is_empty() => Reach::Allowed,
        Answer::NotAllowed => Reach::NotAllowed,
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

    /// Answers one after another, and counts the calls.
    struct Fake(Mutex<Vec<Answer>>, Mutex<usize>);

    impl Fake {
        fn new(mut answers: Vec<Answer>) -> Self {
            answers.reverse();
            Fake(Mutex::new(answers), Mutex::new(0))
        }
        fn calls(&self) -> usize {
            *self.1.lock().unwrap()
        }
    }

    impl ScriptRunner for Fake {
        fn run(&self, _script: &str) -> Answer {
            *self.1.lock().unwrap() += 1;
            self.0.lock().unwrap().pop().unwrap_or(Answer::Unavailable)
        }
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
    fn the_scripts_never_start_a_browser() {
        for browser in Browser::ALL {
            for script in [browser.tab_script(), browser.probe_script()] {
                let guard = script.find("is not running then return").expect("a guard");
                let tell = script.find("tell application").expect("a tell");
                assert!(guard < tell, "the running check comes before the tell");
            }
        }
    }

    #[test]
    fn an_empty_answer_is_asked_once_more() {
        let good = window("normal", "T", "https://example.org/");
        let runner = Fake::new(vec![Answer::Text(String::new()), Answer::Text(good)]);
        assert!(matches!(
            read_tab(&runner, Browser::Chrome),
            TabRead::Tab(_)
        ));
        assert_eq!(runner.calls(), 2);

        let runner = Fake::new(vec![
            Answer::Text(String::new()),
            Answer::Text(String::new()),
        ]);
        assert_eq!(read_tab(&runner, Browser::Chrome), TabRead::Unavailable);
        assert_eq!(runner.calls(), 2);
    }

    #[test]
    fn a_missing_grant_or_no_answer_is_not_asked_again() {
        let runner = Fake::new(vec![Answer::NotAllowed]);
        assert_eq!(read_tab(&runner, Browser::Safari), TabRead::NotAllowed);
        assert_eq!(runner.calls(), 1);
        let runner = Fake::new(vec![Answer::Unavailable]);
        assert_eq!(read_tab(&runner, Browser::Safari), TabRead::Unavailable);
        assert_eq!(runner.calls(), 1);
    }

    #[test]
    fn a_probe_tells_the_grant_apart() {
        let probe_of = |answer| probe(&Fake::new(vec![answer]), Browser::Chrome);
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
}
