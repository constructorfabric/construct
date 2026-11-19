//! The screen signals of a Mac: the seconds since the last input, the front application, the
//! title of its front window, whether the screen is locked or off, and whether the front window
//! is a call window.
//!
//! This is the only macOS code of the agent, kept in a file of its own the way the login service
//! is, and built on macOS only. It uses safe bindings only (the workspace forbids unsafe code).
//!
//! * The idle time, the front application, the lock state and the display state need no
//!   permission.
//! * The window title is read through Accessibility, which the person grants to the agent in
//!   System Settings. The grant is read without asking: a read that the system refuses because
//!   the program is not trusted is how a missing grant shows. A missing grant is reported as
//!   [`Failure::NeedsPermission`] and as a denied grant, never as an empty title that looks
//!   healthy. The system's own prompt cannot be raised without unsafe code, so
//!   [`open_accessibility_settings`] opens the pane where the person adds the agent.
//! * The grant is read from the title of Finder, which always runs: the system answers when the
//!   agent has the grant and refuses with "not trusted" when it has not (the agent's own process
//!   is no probe, it answers "not implemented" either way). For the grant shown, an answer that
//!   says nothing keeps the last clear one, and before any clear answer the grant is reported as
//!   not asked; a read that was refused while the probe says nothing fails instead of storing an
//!   empty title.
//! * Every Accessibility call has a time limit of its own ([`AX_TIMEOUT_SECONDS`]), on top of the
//!   limit the sampler puts on the whole read.
//!
//! A title, a name or a bundle id is never logged or put in an error.

use std::io;
use std::process::Command;
use std::sync::atomic::{AtomicU8, Ordering};
use std::thread;
use std::time::Duration;

use accessibility::{AXAttribute, AXUIElement};
use accessibility_sys::{kAXErrorAPIDisabled, kAXErrorAttributeUnsupported, kAXErrorNoValue};
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_core_graphics::{
    CGDirectDisplayID, CGDisplayIsAsleep, CGEventSource, CGEventSourceStateID, CGEventType,
    CGMainDisplayID, CGSessionCopyCurrentDictionary,
};
use objc2_foundation::{NSDate, NSRunLoop, NSString};

use crate::activity::screen_time::CALL_APPS;
use crate::screen::{App, Failure, Grant, GrantState, Reading, Signals};

/// The name of the grant in the status row.
pub const ACCESSIBILITY: &str = "accessibility";
/// The longest one Accessibility call may wait for the program it asks.
pub const AX_TIMEOUT_SECONDS: f32 = 0.5;
/// The Settings pane where the person adds the agent to Accessibility.
const ACCESSIBILITY_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
/// The name of the default run loop mode (the value of `NSDefaultRunLoopMode`, a constant the
/// bindings give only through an unsafe read).
const DEFAULT_RUN_LOOP_MODE: &str = "kCFRunLoopDefaultMode";
/// How long the main thread runs its loop at a time.
const RUN_LOOP_SECONDS: f64 = 0.1;
/// The bundle id of the application the grant is probed on: Finder always runs in a session and
/// answers Accessibility whatever else is open.
const PROBE_APP: &str = "com.apple.finder";
/// The bundle id of the login window, which is in front while the screen is locked.
const LOGIN_WINDOW: &str = "com.apple.loginwindow";
/// The key of the session dictionary that is true while the screen is locked.
const LOCKED_KEY: &str = "CGSSessionScreenIsLocked";

/// Run the loop of the calling thread, which must be the main thread, until `done` says so.
///
/// The system tells a program that the front application changed through the main thread's run
/// loop. A program that never runs it keeps reading the front application it saw first, whatever
/// thread asks. The agent runs on a thread of its own on a Mac, and the main thread calls this.
pub fn run_main_loop_until(done: impl Fn() -> bool) {
    while !done() {
        let ran = NSRunLoop::currentRunLoop().runMode_beforeDate(
            &NSString::from_str(DEFAULT_RUN_LOOP_MODE),
            &NSDate::dateWithTimeIntervalSinceNow(RUN_LOOP_SECONDS),
        );
        // The call returns at once when the loop has nothing attached yet (before the first
        // read of the front application), so the wait is ours then.
        if !ran {
            thread::sleep(Duration::from_secs_f64(RUN_LOOP_SECONDS));
        }
    }
}

/// Open the Accessibility pane of System Settings. The person's Settings button calls this; the
/// sampler never does.
///
/// # Errors
/// Fails when `open` cannot be run or refuses the address.
pub fn open_accessibility_settings() -> io::Result<()> {
    let status = Command::new("open").arg(ACCESSIBILITY_PANE).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("open"))
    }
}

/// The signals of this Mac.
///
/// It remembers the last clear answer about the grant, so that one probe that says nothing (the
/// probe application busy for a moment) neither alarms a person who granted it nor hides a
/// missing grant that was seen.
#[derive(Default)]
pub struct MacSignals {
    last_trust: AtomicU8,
}

impl MacSignals {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The probe's answer now, remembered when it is clear.
    fn trust_now(&self) -> Trust {
        let now = this_program_trust();
        if now != Trust::Unknown {
            self.last_trust.store(now.to_u8(), Ordering::Relaxed);
        }
        now
    }

    /// The grant to show: the probe's answer when it is clear, else the last clear answer, else
    /// unknown. A read never uses this: a refused read with an unclear probe fails.
    fn trust(&self) -> Trust {
        settle(
            self.trust_now(),
            Trust::from_u8(self.last_trust.load(Ordering::Relaxed)),
        )
    }
}

/// What reading the front window's title gave.
#[derive(Debug, PartialEq, Eq)]
enum Title {
    Read(String),
    /// The program is not trusted for Accessibility.
    NotTrusted,
    /// Trusted, but the application has no window with a title.
    None,
}

/// Whole seconds from the system's idle answer; `None` for a negative or not-a-number one, which
/// is not an idle time (it would read as "just had input").
fn idle_from(seconds: f64) -> Option<u64> {
    if seconds < 0.0 {
        return None;
    }
    Duration::try_from_secs_f64(seconds)
        .ok()
        .map(|d| d.as_secs())
}

/// Seconds since the last keyboard, mouse or other input of the session, `None` when the system
/// gives no usable answer.
#[must_use]
pub fn idle_seconds() -> Option<u64> {
    idle_from(CGEventSource::seconds_since_last_event_type(
        CGEventSourceStateID::CombinedSessionState,
        // The "any input" type, all bits set.
        CGEventType(u32::MAX),
    ))
}

/// The front application and its process id, `None` when no application is in front.
#[must_use]
pub fn front_app() -> Option<(App, i32)> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let pid = app.processIdentifier();
    let name = app
        .localizedName()
        .map(|n| n.to_string())
        .unwrap_or_default();
    let bundle_id = app
        .bundleIdentifier()
        .map(|b| b.to_string())
        .unwrap_or_default();
    if pid <= 0 || (name.is_empty() && bundle_id.is_empty()) {
        return None;
    }
    Some((App { name, bundle_id }, pid))
}

/// Whether the screen is locked or the main display is off.
#[must_use]
pub fn screen_locked_or_off(front_bundle: Option<&str>) -> bool {
    display_asleep(CGMainDisplayID()) || session_locked() || front_bundle == Some(LOGIN_WINDOW)
}

fn display_asleep(display: CGDirectDisplayID) -> bool {
    CGDisplayIsAsleep(display)
}

/// The session dictionary says the screen is locked (the key is absent while it is not).
///
/// The dictionary has no safe typed reader in the bindings, so its own description is read: one
/// `key = value;` line per entry.
fn session_locked() -> bool {
    CGSessionCopyCurrentDictionary().is_some_and(|session| locked_in(&format!("{session:?}")))
}

/// Whether the description of the session dictionary holds the lock key set to true.
fn locked_in(description: &str) -> bool {
    description.lines().any(|line| {
        let line = line.trim();
        line.strip_prefix(LOCKED_KEY)
            .and_then(|rest| rest.trim_start().strip_prefix('='))
            .is_some_and(|value| matches!(value.trim().trim_end_matches(';').trim(), "1" | "true"))
    })
}

/// The title of the front window of the process `pid`, read through Accessibility.
fn window_title(pid: i32) -> Title {
    let app = AXUIElement::application(pid);
    // A failed timeout only means the system's own (long) default stays.
    let _ = app.set_messaging_timeout(AX_TIMEOUT_SECONDS);
    let window = match app.attribute(&AXAttribute::focused_window()) {
        Ok(window) => window,
        Err(e) => return classify(&e),
    };
    match window.attribute(&AXAttribute::title()) {
        Ok(title) => {
            let title = title.to_string();
            if title.is_empty() {
                Title::None
            } else {
                Title::Read(title)
            }
        }
        Err(e) => classify(&e),
    }
}

fn classify(error: &accessibility::Error) -> Title {
    match error {
        accessibility::Error::Ax(code) if *code == kAXErrorAPIDisabled => Title::NotTrusted,
        _ => Title::None,
    }
}

/// Whether the front window of a call application is a call window, from its bundle id and title.
///
/// A call application has windows that are not a call (its home window, a chat list), so the
/// title decides: the call windows of these applications name a meeting or a webinar.
/// `FaceTime`'s window is the call itself. With no title nothing is known, so it is not a call.
#[must_use]
pub fn is_call_window(bundle_id: &str, title: Option<&str>) -> bool {
    let bundle = bundle_id.to_lowercase();
    if !CALL_APPS.contains(&bundle.as_str()) {
        return false;
    }
    if bundle == "com.apple.facetime" {
        return true;
    }
    let title = title.unwrap_or_default().to_lowercase();
    ["meeting", "webinar"]
        .iter()
        .any(|word| title.contains(word))
}

impl Signals for MacSignals {
    fn supported(&self) -> bool {
        true
    }

    fn grants(&self) -> Vec<Grant> {
        let state = match self.trust() {
            Trust::Trusted => GrantState::Granted,
            Trust::NotTrusted => GrantState::Denied,
            Trust::Unknown => GrantState::NotAsked,
        };
        vec![Grant {
            name: ACCESSIBILITY,
            state,
        }]
    }

    fn read(&self) -> Result<Reading, Failure> {
        let front = front_app();
        let bundle = front.as_ref().map(|(app, _)| app.bundle_id.as_str());
        let locked = screen_locked_or_off(bundle);
        // A locked or dark screen counts nothing, so its window is not read: the grant is not in
        // question there, and the reading says only that the screen is locked.
        let title = title_to_read(locked, || front.as_ref().map(|(_, pid)| window_title(*pid)));
        let window_title = front_title(title, || self.trust_now())?;
        let idle_seconds = idle_seconds().ok_or(Failure::Failed("idle"))?;
        let call = !locked && bundle.is_some_and(|b| is_call_window(b, window_title.as_deref()));
        Ok(Reading {
            idle_seconds,
            call,
            locked,
            window_title,
            app: front.map(|(app, _)| app),
            tab: None,
        })
    }
}

/// What the front window gave: nothing is read while the screen is locked or off.
fn title_to_read(locked: bool, read: impl FnOnce() -> Option<Title>) -> Option<Title> {
    if locked { Some(Title::None) } else { read() }
}

/// Whether this program has the Accessibility grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trust {
    Trusted,
    NotTrusted,
    /// The probe gave no clear answer (no probe application, or an error that says nothing about
    /// the grant): neither a grant nor a missing one is claimed.
    Unknown,
}

/// The grant, read from the title of the probe application. The system refuses with "not trusted"
/// when this program has no grant and answers when it has, whatever the front application is.
/// (A program's own process is no probe: it answers "not implemented" in both cases.)
fn this_program_trust() -> Trust {
    let Some(pid) = probe_pid() else {
        return Trust::Unknown;
    };
    let app = AXUIElement::application(pid);
    // A failed timeout only means the system's own (long) default stays.
    let _ = app.set_messaging_timeout(AX_TIMEOUT_SECONDS);
    trust_from(app.attribute(&AXAttribute::title()).err().as_ref())
}

impl Trust {
    fn from_u8(n: u8) -> Self {
        match n {
            1 => Trust::Trusted,
            2 => Trust::NotTrusted,
            _ => Trust::Unknown,
        }
    }

    fn to_u8(self) -> u8 {
        match self {
            Trust::Unknown => 0,
            Trust::Trusted => 1,
            Trust::NotTrusted => 2,
        }
    }
}

/// A clear answer of the probe stands; one that says nothing leaves the last clear answer.
fn settle(probe: Trust, last: Trust) -> Trust {
    if probe == Trust::Unknown { last } else { probe }
}

/// "Attribute not supported" and "no value": the application answered, which an untrusted
/// program never gets (it gets [`kAXErrorAPIDisabled`] first).
const ANSWERED_WITHOUT_VALUE: [i32; 2] = [kAXErrorAttributeUnsupported, kAXErrorNoValue];

fn trust_from(error: Option<&accessibility::Error>) -> Trust {
    match error {
        None => Trust::Trusted,
        Some(accessibility::Error::Ax(code)) if *code == kAXErrorAPIDisabled => Trust::NotTrusted,
        Some(accessibility::Error::Ax(code)) if ANSWERED_WITHOUT_VALUE.contains(code) => {
            Trust::Trusted
        }
        Some(_) => Trust::Unknown,
    }
}

fn probe_pid() -> Option<i32> {
    let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(
        PROBE_APP,
    ));
    let pid = apps.firstObject()?.processIdentifier();
    (pid > 0).then_some(pid)
}

/// The title to store from a read of the front window (`None` when no application is in front).
/// A refusal is a missing grant only when this program is not trusted (`trust` says, and is
/// asked when no application is in front too); an application that refuses by itself while the
/// program is trusted just has no title, and a probe with no clear answer fails the read.
fn front_title(
    title: Option<Title>,
    trust: impl FnOnce() -> Trust,
) -> Result<Option<String>, Failure> {
    match title {
        Some(Title::Read(title)) => return Ok(Some(title)),
        Some(Title::None) => return Ok(None),
        Some(Title::NotTrusted) | None => {}
    }
    match trust() {
        Trust::Trusted => Ok(None),
        Trust::NotTrusted => Err(Failure::NeedsPermission),
        Trust::Unknown => Err(Failure::Failed("accessibility")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `kAXErrorNotImplemented`: what a program's own process answers, with or without a grant.
    const NOT_IMPLEMENTED: i32 = -25208;

    #[test]
    fn idle_time_is_read_without_a_grant() {
        // Any session has had input within a day.
        assert!(idle_seconds().is_some_and(|s| s < 24 * 3600));
    }

    #[test]
    fn an_idle_answer_that_is_not_a_time_is_no_reading() {
        assert_eq!(idle_from(7.9), Some(7));
        assert_eq!(idle_from(0.0), Some(0));
        assert_eq!(idle_from(-1.0), None);
        assert_eq!(idle_from(f64::NAN), None);
        assert_eq!(idle_from(f64::INFINITY), None);
    }

    #[test]
    fn a_refusal_is_a_missing_grant_only_when_this_program_is_not_trusted() {
        let refused = |trust| front_title(Some(Title::NotTrusted), move || trust);
        assert_eq!(refused(Trust::NotTrusted), Err(Failure::NeedsPermission));
        // An application that refuses by itself while this program is trusted has no title.
        assert_eq!(refused(Trust::Trusted), Ok(None));
        // A probe that says nothing claims neither.
        assert_eq!(
            refused(Trust::Unknown),
            Err(Failure::Failed("accessibility"))
        );
    }

    #[test]
    fn no_front_application_and_no_grant_is_still_a_missing_grant() {
        assert_eq!(
            front_title(None, || Trust::NotTrusted),
            Err(Failure::NeedsPermission)
        );
        assert_eq!(front_title(None, || Trust::Trusted), Ok(None));
    }

    #[test]
    fn a_title_or_no_title_needs_no_grant_check() {
        let never = || -> Trust { unreachable!("the grant is not asked") };
        assert_eq!(
            front_title(Some(Title::Read("a".into())), never),
            Ok(Some("a".into()))
        );
        assert_eq!(front_title(Some(Title::None), never), Ok(None));
    }

    #[test]
    fn only_the_not_trusted_error_of_the_probe_is_a_missing_grant() {
        assert_eq!(trust_from(None), Trust::Trusted);
        let refused = accessibility::Error::Ax(kAXErrorAPIDisabled);
        assert_eq!(trust_from(Some(&refused)), Trust::NotTrusted);
        // "Not implemented" (what a program's own process answers) says nothing.
        let other = accessibility::Error::Ax(NOT_IMPLEMENTED);
        assert_eq!(trust_from(Some(&other)), Trust::Unknown);
        assert_eq!(
            trust_from(Some(&accessibility::Error::NotFound)),
            Trust::Unknown
        );
        // An application that answers without a value did answer.
        for code in ANSWERED_WITHOUT_VALUE {
            assert_eq!(
                trust_from(Some(&accessibility::Error::Ax(code))),
                Trust::Trusted
            );
        }
    }

    #[test]
    fn front_app_is_read_without_a_grant() {
        let Some((app, pid)) = front_app() else {
            // A session with no application in front (a login screen) has none to read.
            return;
        };
        assert!(pid > 0);
        assert!(!app.name.is_empty() || !app.bundle_id.is_empty());
    }

    #[test]
    fn the_lock_and_display_reads_run_without_a_grant() {
        // A smoke check only: the state differs from one session to the next (the test may run on
        // a locked screen), so no value is asserted. The parsing of the lock key is tested on the
        // text of a locked Mac below.
        let _ = (session_locked(), screen_locked_or_off(None));
    }

    #[test]
    fn the_lock_key_is_found_only_when_it_is_true() {
        // The lines of the dictionary of a locked Mac, as read from one (the id is left out).
        let locked = concat!(
            "{\n",
            "    CGSSessionScreenIsLocked = 1;\n",
            "    CGSSessionScreenLockedTime = 1795124185;\n",
            "    kCGSSessionOnConsoleKey = 1;\n",
            "}"
        );
        let unlocked = "{\n    kCGSSessionOnConsoleKey = 1;\n}";
        let off = "{\n    CGSSessionScreenIsLocked = 0;\n}";
        assert!(locked_in(locked));
        assert!(!locked_in(unlocked));
        assert!(!locked_in(off));
    }

    #[test]
    fn the_computer_is_supported_and_names_its_one_grant() {
        let signals = MacSignals::new();
        assert!(signals.supported());
        let grants = signals.grants();
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].name, ACCESSIBILITY);
    }

    #[test]
    fn a_read_is_a_reading_or_a_missing_grant_never_an_empty_healthy_one() {
        match MacSignals::new().read() {
            Ok(reading) => {
                // With the grant, a title may be missing (no window), but a front application is
                // never stored without being named.
                if let Some(app) = reading.app {
                    assert!(!app.name.is_empty() || !app.bundle_id.is_empty());
                }
                assert!(reading.tab.is_none());
            }
            // No Finder to probe (a session with none) is the one other answer.
            Err(failure) => assert!(
                failure == Failure::NeedsPermission || failure == Failure::Failed("accessibility")
            ),
        }
    }

    #[test]
    fn a_locked_screen_reads_no_window() {
        let never = || -> Option<Title> { unreachable!("the window is not read") };
        assert_eq!(title_to_read(true, never), Some(Title::None));
        assert_eq!(
            title_to_read(false, || Some(Title::Read("a".into()))),
            Some(Title::Read("a".into()))
        );
    }

    #[test]
    fn a_probe_that_says_nothing_keeps_the_last_clear_answer() {
        assert_eq!(settle(Trust::Unknown, Trust::Trusted), Trust::Trusted);
        assert_eq!(settle(Trust::Unknown, Trust::NotTrusted), Trust::NotTrusted);
        assert_eq!(settle(Trust::Unknown, Trust::Unknown), Trust::Unknown);
        assert_eq!(settle(Trust::NotTrusted, Trust::Trusted), Trust::NotTrusted);
        assert_eq!(settle(Trust::Trusted, Trust::NotTrusted), Trust::Trusted);
        for trust in [Trust::Unknown, Trust::Trusted, Trust::NotTrusted] {
            assert_eq!(Trust::from_u8(trust.to_u8()), trust);
        }
    }

    #[test]
    fn a_refused_read_is_a_missing_grant_and_another_error_is_not() {
        let refused = accessibility::Error::Ax(kAXErrorAPIDisabled);
        assert_eq!(classify(&refused), Title::NotTrusted);
        assert_eq!(classify(&accessibility::Error::NotFound), Title::None);
        assert_eq!(
            classify(&accessibility::Error::Ax(kAXErrorNoValue)),
            Title::None
        );
    }

    #[test]
    fn a_call_window_is_a_call_application_with_a_call_title() {
        assert!(is_call_window("us.zoom.xos", Some("Zoom Meeting")));
        assert!(is_call_window("US.ZOOM.XOS", Some("Zoom Webinar")));
        assert!(!is_call_window("us.zoom.xos", Some("Zoom Workplace")));
        assert!(!is_call_window("us.zoom.xos", None));
        assert!(is_call_window("com.apple.FaceTime", None));
        assert!(!is_call_window("com.apple.Safari", Some("Meeting notes")));
    }
}
