//! Starts the phone endpoint alone, with no database and no Construct host, opens a pairing and
//! prints what a phone needs. It runs until it is stopped, so a phone test can pair against it.
//!
//! ```text
//! cargo run -p construct-agent --example phone_pairing -- [--dir DIR] [--bind ADDR] [--host IP] [--minutes N] [--data FILE]
//! ```
//!
//! `--dir` keeps the certificate and the paired phones (a new temporary directory by default),
//! `--bind` is the address to listen on (`0.0.0.0:0`, any free port, by default), `--host` is the
//! address to put in the QR payload (this computer's address on its network by default) and
//! `--minutes` is how long the pairing stays open (5 by default). With `--control`, send
//! `unpair` on stdin to remove the paired phones, or `stop` to stop the endpoint.
//!
//! With `--data FILE` the endpoint also answers the sync commands (facts, chats and dashboard
//! numbers) from a JSON file of made-up data. The file is read again at every command, so a test
//! may change it while the program runs:
//!
//! ```json
//! {
//!   "zone": "Europe/Berlin",
//!   "facts": {"version": 1, "items": [{"fact_id": "f1", "entity_kind": "skill",
//!             "property": "skills", "value": {"name": "Rust"}, "stored_at": "2026-11-01T10:00:00Z"}]},
//!   "chats": [{"id": "c1", "title": "Hello", "started": "2026-11-01T10:00:00Z",
//!              "items": [{"kind": "message", "role": "user", "time": "2026-11-01T10:00:00Z", "text": "Hi"}]}],
//!   "document": null,
//!   "sessions": [{"tool": "claude", "session": "s1", "project": "/w/app", "ticket": "",
//!                 "minutes": ["2026-11-01T10:00:00Z", "2026-11-01T10:01:00Z"]}]
//! }
//! ```
//!
//! A `null` or missing `document` is the default dashboard; `minutes` are UTC times of minutes
//! with activity. How to stop and start it again is in the README.
//!
//! The output is one `name: value` line each, `payload`, `address`, `code`, `fingerprint` and
//! `dir`, then `ready`, flushed, as the last line before the program waits.

#[cfg(unix)]
mod unix {
    use std::io::{BufRead, Write};
    use std::net::{IpAddr, SocketAddr};
    use std::path::PathBuf;
    use std::time::Duration;

    use std::sync::Arc;

    use construct_agent::activity::rules::{SessionMinutes, minute_number};
    use construct_agent::dashboard::Source;
    use construct_agent::dashboard_numbers::Data as Activity;
    use construct_agent::phone::sync::{
        ChatRead, Data, Engine, FactsRead, Reason, Zone, local_calendar,
    };
    use construct_agent::phone::{Service, lan_address};
    use serde_json::Value;

    /// Made-up data, read from the file again at every call.
    struct FileData(PathBuf);

    impl FileData {
        fn read(&self) -> Result<Value, Reason> {
            let text =
                std::fs::read_to_string(&self.0).map_err(|_| "the data file is unreadable")?;
            serde_json::from_str(&text).map_err(|_| "the data file is not JSON")
        }
    }

    impl Data for FileData {
        fn facts(&self) -> Result<FactsRead, Reason> {
            let file = self.read()?;
            Ok(FactsRead {
                version: file["facts"]["version"].as_u64(),
                facts: file["facts"]["items"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            })
        }

        fn chats(&self) -> Result<Vec<ChatRead>, Reason> {
            let file = self.read()?;
            let text = |chat: &Value, key: &str| chat[key].as_str().unwrap_or_default().to_owned();
            Ok(file["chats"]
                .as_array()
                .map(|chats| {
                    chats
                        .iter()
                        .map(|c| ChatRead {
                            id: text(c, "id"),
                            title: text(c, "title"),
                            started: text(c, "started"),
                            items: c["items"].as_array().cloned().unwrap_or_default(),
                        })
                        .collect()
                })
                .unwrap_or_default())
        }

        fn document(&self) -> Result<Option<Value>, Reason> {
            let file = self.read()?;
            Ok(Some(match &file["document"] {
                Value::Null => construct_agent::dashboard::default_activity(),
                document => document.clone(),
            }))
        }

        fn activity(&self, _: Source) -> Result<Activity, Reason> {
            let file = self.read()?;
            let text = |s: &Value, key: &str| s[key].as_str().unwrap_or_default().to_owned();
            Ok(file["sessions"]
                .as_array()
                .map(|sessions| {
                    sessions
                        .iter()
                        .map(|s| SessionMinutes {
                            tool: text(s, "tool"),
                            session: text(s, "session"),
                            project: text(s, "project"),
                            ticket: text(s, "ticket"),
                            unattended: s["unattended"].as_bool().unwrap_or(false),
                            area: text(s, "area"),
                            collection: text(s, "collection"),
                            minutes: s["minutes"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|m| minute_number(m.as_str()?))
                                .collect(),
                        })
                        .collect::<Vec<_>>()
                        .into()
                })
                .unwrap_or_default())
        }
    }

    fn calendar(data: &FileData) -> Arc<dyn construct_agent::phone::sync::Calendar> {
        let zone = data
            .read()
            .ok()
            .and_then(|f| f["zone"].as_str().map(str::to_owned));
        match zone.and_then(|z| z.parse::<chrono_tz::Tz>().ok().map(|tz| (tz, z))) {
            Some((tz, name)) => Arc::new(Zone(tz, name)),
            None => local_calendar(),
        }
    }

    fn usage() -> ! {
        eprintln!(
            "usage: phone_pairing [--dir DIR] [--bind ADDR] [--host IP] [--minutes N] [--data FILE] [--control]"
        );
        std::process::exit(2);
    }

    pub async fn run() {
        let mut dir: Option<PathBuf> = None;
        let mut bind: SocketAddr = ([0, 0, 0, 0], 0).into();
        let mut host: Option<IpAddr> = None;
        let mut minutes: u64 = 5;
        let mut control = false;
        let mut data: Option<PathBuf> = None;
        let mut args = std::env::args().skip(1);
        while let Some(flag) = args.next() {
            if flag == "--control" {
                control = true;
                continue;
            }
            let Some(value) = args.next() else { usage() };
            match flag.as_str() {
                "--dir" => dir = Some(PathBuf::from(value)),
                "--bind" => bind = value.parse().unwrap_or_else(|_| usage()),
                "--host" => host = Some(value.parse().unwrap_or_else(|_| usage())),
                "--data" => data = Some(PathBuf::from(value)),
                "--minutes" => minutes = value.parse().unwrap_or_else(|_| usage()),
                _ => usage(),
            }
        }
        let dir = dir.unwrap_or_else(|| {
            std::env::temp_dir().join(format!("construct-phone-{}", std::process::id()))
        });
        let host = match host.map_or_else(lan_address, Ok) {
            Ok(host) => host,
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        };
        let service = match Service::start(dir.clone(), bind, Some(host)).await {
            Ok(service) => service,
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        };
        if let Some(file) = data {
            let file = FileData(file);
            let zone = calendar(&file);
            service
                .set_sync(Arc::new(Engine::new(Arc::new(file), zone)))
                .await;
        }
        let offer = match service
            .open_pairing(Duration::from_secs(minutes * 60))
            .await
        {
            Ok(offer) => offer,
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        };
        println!("payload: {}", offer.payload);
        println!("address: {}", offer.address);
        println!("code: {}", offer.code);
        println!("fingerprint: {}", offer.fingerprint);
        println!("dir: {}", dir.display());
        println!("ready");
        let _ = std::io::stdout().flush();
        if control {
            tokio::select! {
                () = commands(&service) => {},
                _ = tokio::signal::ctrl_c() => {},
            }
        } else {
            let _ = tokio::signal::ctrl_c().await;
        }
        service.stop().await;
    }

    async fn commands(service: &Service) {
        let (send, mut receive) = tokio::sync::mpsc::channel(1);
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                if send.blocking_send(line).is_err() {
                    break;
                }
            }
        });
        while let Some(line) = receive.recv().await {
            match line.as_str() {
                "unpair" => {
                    let mut removed = true;
                    for phone in service.phones() {
                        removed &= service.unpair(&phone.id).await == Ok(true);
                    }
                    println!("{}", if removed { "unpaired" } else { "unpair failed" });
                    let _ = std::io::stdout().flush();
                }
                "stop" => break,
                _ => {}
            }
        }
    }
}

#[cfg(unix)]
#[tokio::main]
async fn main() {
    unix::run().await;
}

#[cfg(not(unix))]
fn main() {
    eprintln!("the phone endpoint runs on macOS and Linux");
}
