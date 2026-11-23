//! Starts the phone endpoint alone, with no database and no Construct host, opens a pairing and
//! prints what a phone needs. It runs until it is stopped, so a phone test can pair against it.
//!
//! ```text
//! cargo run -p construct-agent --example phone_pairing -- [--dir DIR] [--bind ADDR] [--host IP] [--minutes N]
//! ```
//!
//! `--dir` keeps the certificate and the paired phones (a new temporary directory by default),
//! `--bind` is the address to listen on (`0.0.0.0:0`, any free port, by default), `--host` is the
//! address to put in the QR payload (this computer's address on its network by default) and
//! `--minutes` is how long the pairing stays open (5 by default). With `--control`, send
//! `unpair` on stdin to remove the paired phones, or `stop` to stop the endpoint.
//!
//! The output is one `name: value` line each, `payload`, `address`, `code`, `fingerprint` and
//! `dir`, then `ready`, flushed, as the last line before the program waits.

#[cfg(unix)]
mod unix {
    use std::io::{BufRead, Write};
    use std::net::{IpAddr, SocketAddr};
    use std::path::PathBuf;
    use std::time::Duration;

    use construct_agent::phone::{Service, lan_address};

    fn usage() -> ! {
        eprintln!(
            "usage: phone_pairing [--dir DIR] [--bind ADDR] [--host IP] [--minutes N] [--control]"
        );
        std::process::exit(2);
    }

    pub async fn run() {
        let mut dir: Option<PathBuf> = None;
        let mut bind: SocketAddr = ([0, 0, 0, 0], 0).into();
        let mut host: Option<IpAddr> = None;
        let mut minutes: u64 = 5;
        let mut control = false;
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
