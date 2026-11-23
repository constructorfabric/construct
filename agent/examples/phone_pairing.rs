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
//! `--minutes` is how long the pairing stays open (5 by default).
//!
//! The output is one `name: value` line each, `payload`, `address`, `code`, `fingerprint` and
//! `dir`, then `ready`, flushed, as the last line before the program waits.

#[cfg(unix)]
mod unix {
    use std::io::Write;
    use std::net::{IpAddr, SocketAddr};
    use std::path::PathBuf;
    use std::time::Duration;

    use construct_agent::phone::{Config, Endpoint, lan_address};

    fn usage() -> ! {
        eprintln!("usage: phone_pairing [--dir DIR] [--bind ADDR] [--host IP] [--minutes N]");
        std::process::exit(2);
    }

    pub async fn run() {
        let mut dir: Option<PathBuf> = None;
        let mut bind: SocketAddr = ([0, 0, 0, 0], 0).into();
        let mut host: Option<IpAddr> = None;
        let mut minutes: u64 = 5;
        let mut args = std::env::args().skip(1);
        while let Some(flag) = args.next() {
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
        let endpoint = match Endpoint::start(Config {
            dir: dir.clone(),
            bind,
        })
        .await
        {
            Ok(endpoint) => endpoint,
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        };
        let offer = match endpoint.open_pairing(host, Duration::from_secs(minutes * 60)) {
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
        let _ = tokio::signal::ctrl_c().await;
        endpoint.stop().await;
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
