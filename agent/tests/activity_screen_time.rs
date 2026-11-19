//! The time-on-screen rules on hand-made runs of samples: no database, no clock, no time zone.

#![cfg(unix)]
// Test code: a failed `unwrap` is a failed test.
#![allow(clippy::unwrap_used)]

use construct_agent::activity::screen_time::{Block, block_of, without_overlap};
use construct_agent::screen::{App, Line, Reading, Tab};

const START: &str = "2026-11-11T10:00:00Z";

fn app(name: &str, bundle: &str) -> App {
    App {
        name: name.into(),
        bundle_id: bundle.into(),
    }
}

fn tab(url: &str, title: &str) -> Tab {
    Tab {
        title: title.into(),
        url: url.into(),
        private: false,
    }
}

fn browser(tab: Tab) -> Reading {
    Reading {
        app: Some(app("Safari", "com.apple.Safari")),
        tab: Some(tab),
        ..Reading::default()
    }
}

fn terminal() -> Reading {
    Reading {
        app: Some(app("Terminal", "com.apple.Terminal")),
        ..Reading::default()
    }
}

/// A run of `samples` samples, five seconds apart, from [`START`].
fn run(samples: u64, reading: Reading) -> Line {
    let span = (samples - 1) * 5;
    Line {
        time: START.into(),
        until: format!("2026-11-11T10:{:02}:{:02}Z", span / 60, span % 60),
        samples,
        reading,
    }
}

fn projects(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| (*n).to_owned()).collect()
}

fn seconds(line: &Line) -> Option<u64> {
    block_of(line, &[]).map(|b| b.seconds)
}

#[test]
fn a_sample_counts_for_its_own_five_seconds() {
    assert_eq!(seconds(&run(1, terminal())), Some(5));
    assert_eq!(seconds(&run(12, terminal())), Some(60));
}

#[test]
fn a_gap_adds_nothing() {
    // Four samples over 45 seconds: the 25 seconds nobody sampled are not counted.
    let mut line = run(4, terminal());
    line.until = "2026-11-11T10:00:45Z".into();
    assert_eq!(seconds(&line), Some(20));
}

#[test]
fn a_run_never_counts_more_than_it_spans() {
    let mut line = run(12, terminal());
    line.until = "2026-11-11T10:00:10Z".into();
    assert_eq!(seconds(&line), Some(15));
}

#[test]
fn the_person_is_away_when_the_last_input_is_older_than_two_minutes() {
    let idle = |idle_seconds| Reading {
        idle_seconds,
        ..terminal()
    };
    // No input for 121 seconds: away from the first sample.
    assert_eq!(seconds(&run(12, idle(121))), None);
    // 120 seconds is not older than two minutes: the first sample counts, the next is away.
    assert_eq!(seconds(&run(12, idle(120))), Some(5));
    // The idle time grows in a run: 30 s before the person is away, the sample at 90 s counts.
    assert_eq!(seconds(&run(40, idle(30))), Some(95));
    assert_eq!(seconds(&run(12, idle(30))), Some(60));
}

#[test]
fn in_a_call_the_person_is_present_without_input() {
    let long = |reading: Reading| Reading {
        idle_seconds: 900,
        ..reading
    };
    let flagged = Reading {
        call: true,
        ..long(terminal())
    };
    assert_eq!(seconds(&run(12, flagged)), Some(60));
    let zoom = Reading {
        app: Some(app("zoom.us", "us.zoom.xos")),
        ..long(Reading::default())
    };
    assert_eq!(seconds(&run(12, zoom)), Some(60));
    for (name, bundle) in [
        ("Microsoft Teams", "com.microsoft.teams2"),
        ("Webex", "Com.Cisco.WebexMeetingsApp"),
        ("FaceTime", "com.apple.FaceTime"),
    ] {
        let reading = Reading {
            app: Some(app(name, bundle)),
            ..long(Reading::default())
        };
        assert_eq!(seconds(&run(12, reading)), Some(60), "{name}");
    }
    let meet = long(browser(tab("https://meet.google.com/abc-defg-hij", "Meet")));
    assert_eq!(seconds(&run(12, meet)), Some(60));
    // The same page on another site is no call.
    let other = long(browser(tab("https://example.com/abc-defg-hij", "Meet")));
    assert_eq!(seconds(&run(12, other)), None);
}

#[test]
fn a_locked_screen_counts_nothing_even_in_a_call() {
    let locked = Reading {
        locked: true,
        ..terminal()
    };
    assert_eq!(seconds(&run(12, locked)), None);
    let locked_call = Reading {
        locked: true,
        call: true,
        idle_seconds: 900,
        ..terminal()
    };
    assert_eq!(seconds(&run(12, locked_call)), None);
}

#[test]
fn nothing_in_front_counts_nothing() {
    assert_eq!(seconds(&run(12, Reading::default())), None);
}

#[test]
fn a_run_with_unreadable_times_counts_nothing() {
    let mut line = run(12, terminal());
    line.until = "yesterday".into();
    assert_eq!(seconds(&line), None);
}

#[test]
fn the_site_is_the_host_without_www() {
    let site = |url: &str| block_of(&run(2, browser(tab(url, ""))), &[]).unwrap().site;
    assert_eq!(site("https://www.Example.com/path?q=1#top"), "example.com");
    assert_eq!(site("http://example.com:8080/"), "example.com");
    assert_eq!(site("https://docs.example.com/"), "docs.example.com");
    assert_eq!(site("https://user:secret@example.com/"), "example.com");
    assert_eq!(site("https://wwwexample.com/"), "wwwexample.com");
    for not_a_page in [
        "",
        "about:blank",
        "file:///home/someone/notes.txt",
        "nonsense",
    ] {
        assert_eq!(site(not_a_page), "", "{not_a_page}");
    }
    // An application that is not a browser has no site.
    assert_eq!(block_of(&run(2, terminal()), &[]).unwrap().site, "");
}

#[test]
fn the_application_is_kept() {
    let block = block_of(&run(2, terminal()), &[]).unwrap();
    assert_eq!(
        (block.app.as_str(), block.start.as_str()),
        ("Terminal", START)
    );
}

fn ticket(reading: Reading) -> String {
    block_of(&run(2, reading), &[]).unwrap().ticket
}

fn titled(window: &str, tab_title: &str, url: &str) -> Reading {
    Reading {
        window_title: Some(window.into()),
        ..browser(tab(url, tab_title))
    }
}

#[test]
fn the_ticket_comes_from_the_address_then_the_tab_title_then_the_window_title() {
    assert_eq!(
        ticket(titled(
            "PRJ-3 window",
            "PRJ-2 tab",
            "https://t.example/browse/PRJ-1"
        )),
        "PRJ-1"
    );
    assert_eq!(
        ticket(titled(
            "PRJ-3 window",
            "PRJ-2 tab",
            "https://t.example/board"
        )),
        "PRJ-2"
    );
    assert_eq!(
        ticket(titled("PRJ-3 window", "a tab", "https://t.example/board")),
        "PRJ-3"
    );
    assert_eq!(
        ticket(titled("a window", "a tab", "https://t.example/board")),
        ""
    );
}

#[test]
fn a_place_with_several_ids_gives_way_to_the_next() {
    assert_eq!(
        ticket(titled(
            "PRJ-3",
            "PRJ-1 and PRJ-2",
            "https://t.example/board"
        )),
        "PRJ-3"
    );
    // Several ids everywhere: none, the rule never picks one.
    assert_eq!(
        ticket(titled(
            "PRJ-3 PRJ-4",
            "PRJ-1 and PRJ-2",
            "https://t.example/board"
        )),
        ""
    );
    // The same id twice is one id.
    assert_eq!(
        ticket(titled("", "PRJ-1 is PRJ-1", "https://t.example/board")),
        "PRJ-1"
    );
}

#[test]
fn the_ticket_of_a_window_that_is_not_a_tab_comes_from_its_title() {
    let reading = Reading {
        window_title: Some("fix PRJ-9 — editor".into()),
        ..terminal()
    };
    assert_eq!(ticket(reading), "PRJ-9");
    // Words that look like an id are none, as for a session.
    let reading = Reading {
        window_title: Some("UTF-8 notes".into()),
        ..terminal()
    };
    assert_eq!(ticket(reading), "");
}

fn project(reading: Reading, known: &[&str]) -> String {
    block_of(&run(2, reading), &projects(known))
        .unwrap()
        .project
}

#[test]
fn a_project_is_found_as_a_whole_word_when_exactly_one_is() {
    let known = ["construct", "dashboard"];
    // None.
    assert_eq!(
        project(titled("notes", "weather", "https://example.com/"), &known),
        ""
    );
    // One, in the address, in a tab title, in a window title; the case does not matter.
    assert_eq!(
        project(
            titled("w", "t", "https://git.example/org/construct/pulls"),
            &known
        ),
        "construct"
    );
    assert_eq!(
        project(
            titled("w", "Construct — pulls", "https://example.com/"),
            &known
        ),
        "construct"
    );
    assert_eq!(
        project(
            titled("work on dashboard", "t", "https://example.com/"),
            &known
        ),
        "dashboard"
    );
    // The same project in two places is one project.
    assert_eq!(
        project(
            titled("construct", "construct", "https://example.com/construct"),
            &known
        ),
        "construct"
    );
}

#[test]
fn several_projects_give_none() {
    let known = ["construct", "dashboard"];
    assert_eq!(
        project(
            titled("dashboard", "construct", "https://example.com/"),
            &known
        ),
        ""
    );
    assert_eq!(
        project(
            titled("w", "construct and dashboard", "https://example.com/"),
            &known
        ),
        ""
    );
}

#[test]
fn a_project_inside_a_longer_word_is_not_found() {
    let known = ["construct"];
    for text in [
        "constructor",
        "reconstruct",
        "construct-browser",
        "my_construct",
        "construct2",
    ] {
        assert_eq!(
            project(titled(text, "t", "https://example.com/"), &known),
            "",
            "{text}"
        );
    }
    // Where a longer name is known too, the one that fits is the one found.
    assert_eq!(
        project(
            titled("construct-browser", "t", "https://example.com/"),
            &["construct", "construct-browser"]
        ),
        "construct-browser"
    );
}

#[test]
fn a_very_short_project_name_is_never_matched() {
    assert_eq!(
        project(titled("go to ui", "t", "https://example.com/"), &["ui"]),
        ""
    );
}

#[test]
fn a_private_window_keeps_only_the_application() {
    let reading = Reading {
        window_title: Some("PRJ-1 construct".into()),
        tab: Some(Tab {
            title: "PRJ-2 construct".into(),
            url: "https://example.com/construct/PRJ-3".into(),
            private: true,
        }),
        ..browser(tab("", ""))
    };
    let block = block_of(&run(12, reading), &projects(&["construct"])).unwrap();
    assert_eq!(
        block,
        Block {
            start: START.into(),
            seconds: 60,
            app: "Safari".into(),
            site: String::new(),
            project: String::new(),
            ticket: String::new(),
        }
    );
}

#[test]
fn a_password_manager_keeps_no_title() {
    let reading = Reading {
        app: Some(app("1Password", "com.1password.1password")),
        window_title: Some("PRJ-1 construct login".into()),
        ..Reading::default()
    };
    let block = block_of(&run(2, reading), &projects(&["construct"])).unwrap();
    assert_eq!(block.app, "1Password");
    assert_eq!((block.project.as_str(), block.ticket.as_str()), ("", ""));
    // The same title in an application that is no password manager is read.
    let reading = Reading {
        app: Some(app("Notes", "com.apple.Notes")),
        window_title: Some("PRJ-1 construct login".into()),
        ..Reading::default()
    };
    let block = block_of(&run(2, reading), &projects(&["construct"])).unwrap();
    assert_eq!(
        (block.project.as_str(), block.ticket.as_str()),
        ("construct", "PRJ-1")
    );
}

#[test]
fn a_block_holds_no_title_and_no_address() {
    let reading = titled(
        "window words",
        "tab words",
        "https://www.example.com/secret/path?token=abc",
    );
    let block = block_of(&run(2, reading), &[]).unwrap();
    let shown = format!("{block:?}");
    for leaked in ["window words", "tab words", "secret", "token", "/path"] {
        assert!(!shown.contains(leaked), "{leaked} is in {shown}");
    }
}

fn block(start: &str, seconds: u64) -> Block {
    Block {
        start: start.into(),
        seconds,
        app: "Terminal".into(),
        site: String::new(),
        project: String::new(),
        ticket: String::new(),
    }
}

#[test]
fn blocks_never_overlap() {
    let blocks = vec![
        block("2026-11-11T10:01:00Z", 60),
        block("2026-11-11T10:00:00Z", 90),
        block("2026-11-11T10:03:00Z", 10),
    ];
    let cut = without_overlap(blocks);
    let seen: Vec<(&str, u64)> = cut.iter().map(|b| (b.start.as_str(), b.seconds)).collect();
    // The first block is cut where the second starts; the others fit.
    assert_eq!(
        seen,
        [
            ("2026-11-11T10:00:00Z", 60),
            ("2026-11-11T10:01:00Z", 60),
            ("2026-11-11T10:03:00Z", 10),
        ]
    );
}

#[test]
fn two_blocks_that_start_at_one_instant_leave_one() {
    let cut = without_overlap(vec![
        block("2026-11-11T10:00:00.000Z", 30),
        block("2026-11-11T10:00:00Z", 30),
    ]);
    assert_eq!(cut.len(), 1);
    assert_eq!(cut[0].seconds, 30);
}
