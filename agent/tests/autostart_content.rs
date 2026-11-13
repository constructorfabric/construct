//! Content of the generated service files and the Linux install/uninstall flow
//! against a fake `systemctl`.

use std::cell::RefCell;
use std::io;
use std::path::Path;
use std::rc::Rc;

use construct_agent::autostart::{self, linux, macos, windows, Autostart, CommandRunner};

#[derive(Clone, Default)]
struct Recorder(Rc<RefCell<Vec<String>>>);

impl CommandRunner for Recorder {
    fn run(&self, program: &str, args: &[&str]) -> io::Result<()> {
        self.0
            .borrow_mut()
            .push(format!("{program} {}", args.join(" ")));
        Ok(())
    }
}

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
    let dir = std::env::temp_dir().join(format!("construct-agent-test-{}", std::process::id()));
    let rec = Recorder::default();
    let sd = linux::SystemdUser::new(dir.join("systemd/user"), Box::new(rec.clone()));

    assert!(!sd.is_installed());
    sd.install(Path::new("/usr/bin/construct-agent")).unwrap();
    assert!(sd.is_installed());
    let written = std::fs::read_to_string(sd.unit_path()).unwrap();
    assert_eq!(
        written,
        linux::unit_file(Path::new("/usr/bin/construct-agent"))
    );
    assert_eq!(
        *rec.0.borrow(),
        [
            "systemctl --user daemon-reload",
            "systemctl --user enable --now construct-agent.service",
            "systemctl --user restart construct-agent.service"
        ]
    );

    rec.0.borrow_mut().clear();
    sd.uninstall().unwrap();
    assert!(!sd.is_installed());
    assert!(!sd.unit_path().exists());
    assert_eq!(
        *rec.0.borrow(),
        [
            "systemctl --user stop construct-agent.service",
            "systemctl --user disable construct-agent.service",
            "systemctl --user daemon-reload"
        ]
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linux_uninstall_stops_the_service_even_when_the_unit_file_is_gone() {
    let dir = std::env::temp_dir().join(format!("construct-agent-gone-{}", std::process::id()));
    let rec = Recorder::default();
    let sd = linux::SystemdUser::new(dir.join("systemd/user"), Box::new(rec.clone()));
    assert!(!sd.is_installed());
    sd.uninstall().unwrap();
    assert!(rec
        .0
        .borrow()
        .contains(&"systemctl --user stop construct-agent.service".to_string()));
}

#[test]
fn macos_uninstall_boots_the_job_out_even_when_the_plist_is_gone() {
    let home = std::env::temp_dir().join(format!("construct-agent-macgone-{}", std::process::id()));
    let rec = Recorder::default();
    let agent = macos::LaunchAgent::new(home, "1234".into(), Box::new(rec.clone()));
    agent.uninstall().unwrap();
    assert_eq!(
        *rec.0.borrow(),
        ["launchctl bootout gui/1234/cf.construct.agent"]
    );
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
fn uid_comes_only_from_a_successful_numeric_id_output() {
    assert_eq!(autostart::parse_uid(true, "501\n").unwrap(), "501");
    assert!(autostart::parse_uid(true, "").is_err());
    assert!(autostart::parse_uid(true, "root").is_err());
    assert!(autostart::parse_uid(false, "501").is_err());
}

#[test]
fn windows_install_is_refused_until_the_socket_exists() {
    let rec = Recorder::default();
    let key = windows::RunKey::new(Box::new(rec.clone()));
    let err = key
        .install(Path::new(r"C:\\construct-agent.exe"))
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Unsupported);
    assert!(
        rec.0.borrow().is_empty(),
        "nothing is written to the Run key"
    );
}

#[test]
fn macos_uses_the_given_uid_for_launchctl() {
    let home = std::env::temp_dir().join(format!("construct-agent-mac-{}", std::process::id()));
    let rec = Recorder::default();
    let agent = macos::LaunchAgent::new(home.clone(), "1234".into(), Box::new(rec.clone()));
    agent
        .install(Path::new("/usr/bin/construct-agent"))
        .unwrap();
    let calls = rec.0.borrow();
    assert_eq!(calls[0], "launchctl bootout gui/1234/cf.construct.agent");
    assert!(
        calls[1].starts_with("launchctl bootstrap gui/1234 "),
        "{}",
        calls[1]
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn replies_are_valid_json_even_with_control_characters() {
    let reply = construct_agent::ipc::unknown_command_response("a\u{1b}b\"c");
    let v: serde_json::Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(v["command"], "a\u{1b}b\"c");
}
