//! Content of the generated service and plist files, and install and uninstall against a
//! recording fake of `systemctl` and `launchctl`.

mod common;

use std::path::Path;

use construct_agent::autostart::{self, Autostart, linux, macos, windows};

use common::{LINUX_INSTALL, Scenario};

#[test]
fn linux_unit_file_restarts_and_starts_at_login() {
    let unit = linux::unit_file(Path::new("/opt/Construct Browser/construct-agent"));
    assert!(unit.contains("ExecStart=\"/opt/Construct Browser/construct-agent\" run\n"));
    assert!(unit.contains("Restart=always\n"));
    assert!(unit.contains("WantedBy=default.target\n"));
    assert!(unit.contains("Type=simple\n"));
    assert!(unit.contains("StartLimitIntervalSec=0\n"));
}

#[test]
fn linux_unit_file_keeps_a_line_break_in_the_path_inside_the_line() {
    let unit = linux::unit_file(Path::new("/opt/a\nExecStart=/bin/evil\r/construct-agent"));
    assert!(unit.contains(r#"ExecStart="/opt/a\nExecStart=/bin/evil\r/construct-agent" run"#));
    assert_eq!(unit.matches("ExecStart=").count(), 2, "{unit}"); // only the injected text, inside the quotes
    assert!(!unit.lines().any(|l| l.starts_with("ExecStart=/bin/evil")));
}

#[test]
fn linux_unit_file_escapes_dollar_so_systemd_does_not_expand_it() {
    let unit = linux::unit_file(Path::new("/opt/a$b/construct-agent"));
    assert!(
        unit.contains(r#"ExecStart="/opt/a$$b/construct-agent" run"#),
        "{unit}"
    );
}

#[test]
fn linux_unit_file_escapes_percent_quote_and_backslash() {
    let unit = linux::unit_file(Path::new(r#"/opt/100%/a"b\c/construct-agent"#));
    assert!(
        unit.contains(r#"ExecStart="/opt/100%%/a\"b\\c/construct-agent" run"#),
        "{unit}"
    );
}

#[test]
fn macos_plist_keeps_alive_and_runs_at_load() {
    let plist = macos::plist(
        Path::new("/Applications/Construct & Co/construct-agent"),
        Path::new("/Users/u/Library/Logs/a.log"),
    );
    assert!(plist.contains("<string>cf.construct.agent</string>"));
    assert!(plist.contains("<string>/Applications/Construct &amp; Co/construct-agent</string>\n        <string>run</string>"));
    assert!(plist.contains("<key>KeepAlive</key>\n    <true/>"));
    assert!(plist.contains("<key>RunAtLoad</key>\n    <true/>"));
    assert!(plist.contains(
        "<key>StandardErrorPath</key>\n    <string>/Users/u/Library/Logs/a.log</string>"
    ));
}

#[test]
fn linux_install_writes_unit_and_enables_it_then_uninstall_removes_it() {
    let s = Scenario::new();
    let sd = s.linux();

    assert!(!sd.is_installed());
    sd.install(&s.exe).unwrap();
    assert!(sd.is_installed());
    let written = std::fs::read_to_string(sd.unit_path()).unwrap();
    assert_eq!(written, linux::unit_file(&s.exe));
    assert_eq!(s.take_calls(), LINUX_INSTALL);

    sd.uninstall().unwrap();
    assert!(!sd.is_installed());
    assert!(!sd.unit_path().exists());
    assert_eq!(
        s.take_calls(),
        [
            "systemctl --user stop construct-agent.service",
            "systemctl --user disable construct-agent.service",
            "systemctl --user daemon-reload"
        ]
    );
}

#[test]
fn linux_uninstall_stops_the_service_even_when_the_unit_file_is_gone() {
    let s = Scenario::new();
    let sd = s.linux();
    assert!(!sd.is_installed());
    sd.uninstall().unwrap();
    assert!(
        s.take_calls()
            .contains(&"systemctl --user stop construct-agent.service".to_string())
    );
}

#[test]
fn linux_uninstall_reports_a_failed_stop_of_an_installed_unit_and_keeps_the_file() {
    let s = Scenario::new();
    let sd = s.linux();
    sd.install(&s.exe).unwrap();
    s.rec.fail_on(&[""]);
    assert!(sd.uninstall().is_err());
    assert!(
        sd.is_installed(),
        "the unit stays so uninstall can be retried"
    );
}

#[test]
fn macos_uninstall_reports_a_refused_bootout_of_a_loaded_job_and_keeps_the_file() {
    let s = Scenario::new();
    let agent = s.macos();
    agent.install(&s.exe).unwrap();
    // `bootout` is refused while `print` still finds the job: it is loaded.
    s.rec.fail_on(&["bootout"]);
    assert!(agent.uninstall().is_err());
    assert!(agent.is_installed());
}

#[test]
fn macos_uninstall_removes_the_plist_of_a_job_that_is_not_loaded() {
    let s = Scenario::new();
    let agent = s.macos();
    agent.install(&s.exe).unwrap();
    // Every launchctl call fails, `print` included: nothing is loaded.
    s.rec.fail_on(&[""]);
    agent.uninstall().unwrap();
    assert!(!agent.is_installed());
}

#[test]
fn macos_uninstall_boots_the_job_out_even_when_the_plist_is_gone() {
    let s = Scenario::new();
    s.macos().uninstall().unwrap();
    assert_eq!(
        s.take_calls(),
        ["launchctl bootout gui/1234/cf.construct.agent"]
    );
}

#[test]
fn uid_comes_only_from_a_successful_numeric_id_output() {
    assert_eq!(autostart::parse_uid(true, "501\n").unwrap(), "501");
    assert!(autostart::parse_uid(true, "").is_err());
    assert!(autostart::parse_uid(true, "root").is_err());
    assert!(autostart::parse_uid(false, "501").is_err());
}

#[test]
fn windows_run_key_command() {
    let exe = Path::new(r"C:\Program Files\Construct\construct-agent.exe");
    assert_eq!(
        windows::run_command(exe),
        r#""C:\Program Files\Construct\construct-agent.exe" run"#
    );
    assert_eq!(
        windows::reg_add_args(exe),
        [
            "add",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "construct-agent",
            "/t",
            "REG_SZ",
            "/d",
            r#""C:\Program Files\Construct\construct-agent.exe" run"#,
            "/f"
        ]
    );
}

#[test]
fn windows_install_is_refused_until_the_socket_exists() {
    let s = Scenario::new();
    let key = windows::RunKey::new(Box::new(s.rec.clone()));
    let err = key
        .install(Path::new(r"C:\\construct-agent.exe"))
        .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    assert!(
        s.take_calls().is_empty(),
        "nothing is written to the Run key"
    );
}

#[test]
fn windows_ensure_is_refused_until_the_socket_exists() {
    let s = Scenario::new();
    let key = windows::RunKey::new(Box::new(s.rec.clone()));
    let err = key
        .ensure(Path::new(r"C:\\construct-agent.exe"))
        .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    assert!(
        s.take_calls().is_empty(),
        "nothing is written to the Run key"
    );
}

#[test]
fn macos_uses_the_given_uid_for_launchctl() {
    let s = Scenario::new();
    s.macos().install(&s.exe).unwrap();
    let calls = s.take_calls();
    assert_eq!(calls[0], "launchctl bootout gui/1234/cf.construct.agent");
    assert_eq!(calls[1], "launchctl print gui/1234/cf.construct.agent");
    assert!(
        calls[2].starts_with("launchctl bootstrap gui/1234 "),
        "{}",
        calls[2]
    );
}

#[test]
fn replies_are_valid_json_even_with_control_characters() {
    let reply = construct_agent::ipc::unknown_command_response("a\u{1b}b\"c");
    let v: serde_json::Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(v["error"], "unknown command");
    assert!(v.get("command").is_none());
}
