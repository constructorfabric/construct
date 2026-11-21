//! `ensure`: when the agent is registered and started again, and when it is left alone.

mod common;

use std::time::Duration;

use construct_agent::autostart::Autostart;

use common::{LINUX_ENABLE, LINUX_INSTALL, MACOS_INSTALL, MACOS_PRINT, Scenario};

#[test]
fn linux_first_start_installs_and_restarts() {
    let s = Scenario::new();
    s.linux().ensure(&s.exe).unwrap();
    assert_eq!(s.take_calls(), LINUX_INSTALL);
}

#[test]
fn linux_same_binary_enables_again_and_restarts_nothing() {
    let s = Scenario::new();
    let sd = s.linux();
    sd.ensure(&s.exe).unwrap();
    s.take_calls();
    sd.ensure(&s.exe).unwrap();
    assert_eq!(s.take_calls(), LINUX_ENABLE);
}

#[test]
fn linux_moved_binary_registers_again() {
    let s = Scenario::new();
    let sd = s.linux();
    sd.ensure(&s.exe).unwrap();
    s.take_calls();
    let moved = s.moved_binary();
    sd.ensure(&moved).unwrap();
    assert_eq!(s.take_calls(), LINUX_INSTALL);
    let unit = std::fs::read_to_string(sd.unit_path()).unwrap();
    assert!(unit.contains(&moved.display().to_string()));
}

#[test]
fn linux_binary_replaced_at_the_same_path_restarts_even_with_the_same_size_and_mtime() {
    let s = Scenario::new();
    let sd = s.linux();
    sd.ensure(&s.exe).unwrap();
    s.replace_binary_keeping_mtime();
    s.take_calls();
    sd.ensure(&s.exe).unwrap();
    assert_eq!(s.take_calls(), LINUX_INSTALL);
    sd.ensure(&s.exe).unwrap();
    assert_eq!(
        s.take_calls(),
        LINUX_ENABLE,
        "the next start leaves it alone"
    );
}

#[test]
fn linux_same_content_with_a_newer_mtime_restarts_nothing() {
    let s = Scenario::new();
    let sd = s.linux();
    sd.ensure(&s.exe).unwrap();
    s.touch_binary();
    s.take_calls();
    sd.ensure(&s.exe).unwrap();
    assert_eq!(s.take_calls(), LINUX_ENABLE);
}

#[test]
fn linux_failed_enable_on_a_first_start_installs_again() {
    let s = Scenario::new();
    let sd = s.linux();
    s.rec.fail_on(&["enable"]);
    assert!(sd.ensure(&s.exe).is_err());
    s.rec.fail_on(&[]);
    s.take_calls();
    sd.ensure(&s.exe).unwrap();
    assert_eq!(s.take_calls(), LINUX_INSTALL);
}

#[test]
fn linux_failed_restart_of_a_replaced_binary_is_retried_and_keeps_the_old_registration() {
    let s = Scenario::new();
    let sd = s.linux();
    sd.ensure(&s.exe).unwrap();
    let old_unit = std::fs::read_to_string(sd.unit_path()).unwrap();

    s.replace_binary_keeping_mtime();
    s.rec.fail_on(&["restart"]);
    assert!(sd.ensure(&s.exe).is_err());
    assert_eq!(
        std::fs::read_to_string(sd.unit_path()).unwrap(),
        old_unit,
        "the unit of a working registration is not deleted"
    );

    s.rec.fail_on(&[]);
    s.take_calls();
    sd.ensure(&s.exe).unwrap();
    assert_eq!(s.take_calls(), LINUX_INSTALL);
}

#[test]
fn linux_uninstall_removes_the_unit_and_the_stamp_and_keeps_the_lock_file() {
    let s = Scenario::new();
    let sd = s.linux();
    sd.ensure(&s.exe).unwrap();
    sd.uninstall().unwrap();
    let mut left: Vec<_> = std::fs::read_dir(s.dir.join("systemd/user"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["construct-agent.service.lock"]);
}

#[test]
fn linux_failed_start_after_the_same_bytes_moved_is_retried() {
    let s = Scenario::new();
    let sd = s.linux();
    sd.ensure(&s.exe).unwrap();
    let moved = s.moved_binary();
    s.rec.fail_on(&["restart"]);
    assert!(sd.ensure(&moved).is_err());
    s.rec.fail_on(&[]);
    s.take_calls();
    sd.ensure(&moved).unwrap();
    assert_eq!(s.take_calls(), LINUX_INSTALL);
}

#[test]
fn macos_first_start_installs() {
    let s = Scenario::new();
    let agent = s.macos();
    agent.ensure(&s.exe).unwrap();
    assert_eq!(s.take_macos_calls(), MACOS_INSTALL);
    let plist = std::fs::read_to_string(agent.plist_path()).unwrap();
    assert!(plist.contains(&s.exe.display().to_string()));
}

#[test]
fn macos_same_binary_with_a_loaded_job_only_checks() {
    let s = Scenario::new();
    let agent = s.macos();
    agent.ensure(&s.exe).unwrap();
    s.take_calls();
    agent.ensure(&s.exe).unwrap();
    assert_eq!(s.take_calls(), [MACOS_PRINT]);
}

#[test]
fn macos_matching_registration_whose_job_is_not_loaded_loads_it() {
    let s = Scenario::new();
    let agent = s.macos();
    agent.ensure(&s.exe).unwrap();
    s.take_calls();
    s.rec.fail_on(&["print"]);
    agent.ensure(&s.exe).unwrap();
    assert_eq!(
        s.take_macos_calls(),
        [MACOS_PRINT, "launchctl bootstrap gui/1234 "]
    );
}

#[test]
fn macos_bootstrap_that_fails_for_a_job_that_is_not_loaded_is_reported() {
    let s = Scenario::new();
    let agent = s.macos();
    agent.ensure(&s.exe).unwrap();
    s.take_calls();
    s.rec.fail_on(&[""]);
    assert!(agent.ensure(&s.exe).is_err());
    assert_eq!(
        s.take_macos_calls(),
        [MACOS_PRINT, "launchctl bootstrap gui/1234 "]
    );
}

#[test]
fn macos_failed_bootstrap_on_a_first_start_installs_again() {
    let s = Scenario::new();
    let agent = s.macos();
    s.rec.fail_on(&["bootstrap"]);
    assert!(agent.ensure(&s.exe).is_err());
    s.rec.fail_on(&[]);
    s.take_calls();
    agent.ensure(&s.exe).unwrap();
    assert_eq!(s.take_macos_calls(), MACOS_INSTALL);
}

#[test]
fn macos_moved_binary_registers_again() {
    let s = Scenario::new();
    let agent = s.macos();
    agent.ensure(&s.exe).unwrap();
    s.take_calls();
    let moved = s.moved_binary();
    agent.ensure(&moved).unwrap();
    assert_eq!(s.take_macos_calls(), MACOS_INSTALL);
    let plist = std::fs::read_to_string(agent.plist_path()).unwrap();
    assert!(plist.contains(&moved.display().to_string()));
}

#[test]
fn macos_binary_replaced_at_the_same_path_restarts_even_with_the_same_size_and_mtime() {
    let s = Scenario::new();
    let agent = s.macos();
    agent.ensure(&s.exe).unwrap();
    s.replace_binary_keeping_mtime();
    s.take_calls();
    agent.ensure(&s.exe).unwrap();
    assert_eq!(s.take_macos_calls(), MACOS_INSTALL);
    agent.ensure(&s.exe).unwrap();
    assert_eq!(s.take_calls(), [MACOS_PRINT]);
}

#[test]
fn macos_same_content_with_a_newer_mtime_restarts_nothing() {
    let s = Scenario::new();
    let agent = s.macos();
    agent.ensure(&s.exe).unwrap();
    s.touch_binary();
    s.take_calls();
    agent.ensure(&s.exe).unwrap();
    assert_eq!(s.take_calls(), [MACOS_PRINT]);
}

#[test]
fn macos_install_reports_a_refused_bootout_of_a_loaded_job_and_bootstraps_nothing() {
    let s = Scenario::new();
    s.rec.fail_on(&["bootout"]);
    assert!(s.macos().install(&s.exe).is_err());
    assert!(!s.take_calls().iter().any(|c| c.contains("bootstrap")));
}

#[test]
fn macos_bootstrap_waits_until_the_old_job_is_gone() {
    let s = Scenario::new();
    let agent = s.macos();
    s.rec.job_lingers_for(2);
    agent.install(&s.exe).unwrap();
    assert_eq!(
        s.take_macos_calls(),
        [
            "launchctl bootout gui/1234/cf.construct.agent",
            MACOS_PRINT,
            MACOS_PRINT,
            MACOS_PRINT,
            "launchctl bootstrap gui/1234 ",
        ]
    );
    agent.ensure(&s.exe).unwrap();
    assert_eq!(s.take_calls(), [MACOS_PRINT]);
}

#[test]
fn macos_job_that_never_goes_down_is_reported_and_bootstraps_nothing() {
    let s = Scenario::new();
    let agent = s.macos_waiting(Duration::from_millis(300));
    s.rec.job_lingers_for(usize::MAX);
    assert!(agent.install(&s.exe).is_err());
    assert!(!s.take_calls().iter().any(|c| c.contains("bootstrap")));
}

#[test]
fn macos_install_goes_on_when_bootout_fails_because_the_job_is_not_loaded() {
    let s = Scenario::new();
    s.rec.fail_on(&["bootout", "print"]);
    s.macos().install(&s.exe).unwrap();
    assert!(s.take_calls().iter().any(|c| c.contains("bootstrap")));
}

#[test]
fn ensure_of_a_binary_that_cannot_be_read_is_an_error_and_changes_nothing() {
    let s = Scenario::new();
    let missing = s.dir.join("gone");
    assert!(s.linux().ensure(&missing).is_err());
    assert!(s.macos().ensure(&missing).is_err());
    assert!(s.take_calls().is_empty());
}
