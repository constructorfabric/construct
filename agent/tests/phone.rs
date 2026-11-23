//! The phone endpoint: pairing, the encrypted channel, silence towards strangers, versions.
#![cfg(unix)]
// Helpers sit outside `#[test]` functions, where `clippy.toml` does not allow `unwrap`.
#![allow(clippy::unwrap_used)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use construct_agent::phone::pairing::{Role, proof};
use construct_agent::phone::{Config, Endpoint, PROTOCOL, PairingOffer, Service, fingerprint_of};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme, version};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

const LOCAL: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const TTL: Duration = Duration::from_secs(60);

struct Dir(PathBuf);

impl Dir {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "construct-phone-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        Self(path)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn start(dir: &Dir) -> Endpoint {
    Endpoint::start(Config {
        dir: dir.0.clone(),
        bind: SocketAddr::new(LOCAL, 0),
    })
    .await
    .unwrap()
}

/// What a phone does with a server certificate: take it when its fingerprint is the pinned one
/// (or any, when nothing is pinned yet), whatever the address or name, and write down what it saw.
#[derive(Debug)]
struct Pin {
    pinned: Option<[u8; 32]>,
    seen: Mutex<Option<[u8; 32]>>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pin {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let fingerprint = fingerprint_of(end_entity);
        *self.seen.lock().unwrap() = Some(fingerprint);
        match self.pinned {
            Some(pinned) if pinned != fingerprint => Err(rustls::Error::General(
                "certificate is not the pinned one".into(),
            )),
            _ => Ok(ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

struct Phone {
    reader: BufReader<ReadHalf<TlsStream<TcpStream>>>,
    writer: WriteHalf<TlsStream<TcpStream>>,
    seen: [u8; 32],
}

fn fp_bytes(hex_text: &str) -> [u8; 32] {
    hex::decode(hex_text).unwrap().try_into().unwrap()
}

impl Phone {
    /// Connect and pin `pinned`; with `None` the phone trusts the first certificate it sees.
    async fn connect(addr: SocketAddr, pinned: Option<[u8; 32]>) -> Option<Self> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let pin = Arc::new(Pin {
            pinned,
            seen: Mutex::new(None),
            provider: Arc::clone(&provider),
        });
        let config = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&version::TLS13])
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::clone(&pin) as Arc<dyn ServerCertVerifier>)
            .with_no_client_auth();
        let stream = TcpStream::connect(addr).await.ok()?;
        let tls = TlsConnector::from(Arc::new(config))
            .connect(ServerName::IpAddress(addr.ip().into()), stream)
            .await
            .ok()?;
        let seen = (*pin.seen.lock().unwrap())?;
        let (reader, writer) = tokio::io::split(tls);
        Some(Self {
            reader: BufReader::new(reader),
            writer,
            seen,
        })
    }

    async fn send(&mut self, value: &Value) {
        let mut line = value.to_string();
        line.push('\n');
        // The agent may have closed already; the answer, or its absence, is what a test reads.
        let _ = self.writer.write_all(line.as_bytes()).await;
        let _ = self.writer.flush().await;
    }

    /// The next JSON line, or `None` when the agent closed without sending one.
    async fn recv(&mut self) -> Option<Value> {
        let mut line = String::new();
        let read = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::io::AsyncBufReadExt::read_line(&mut self.reader, &mut line),
        )
        .await
        .unwrap_or_else(|_| panic!("the agent neither answered nor closed"));
        match read {
            Ok(n) if n > 0 => Some(serde_json::from_str(&line).unwrap()),
            _ => None,
        }
    }
}

fn pair_message(secret: &[u8], seen: &[u8; 32], version: u64, name: &str) -> Value {
    json!({"v": version, "pair": {"proof": proof(secret, Role::Phone, seen), "name": name}})
}

/// Pair with the long secret from the QR payload and give back the token.
async fn pair(addr: SocketAddr, offer: &PairingOffer) -> String {
    pair_named(addr, offer, "Test phone").await
}

async fn pair_named(addr: SocketAddr, offer: &PairingOffer, name: &str) -> String {
    let secret = qr_secret(offer);
    let mut phone = Phone::connect(addr, Some(fp_bytes(&offer.fingerprint)))
        .await
        .unwrap();
    let seen = phone.seen;
    phone
        .send(&pair_message(&secret, &seen, PROTOCOL, name))
        .await;
    let reply = phone.recv().await.unwrap();
    assert_eq!(reply["paired"], true);
    reply["token"].as_str().unwrap().to_owned()
}

fn qr_secret(offer: &PairingOffer) -> Vec<u8> {
    let query = offer.payload.split_once('?').unwrap().1;
    let hex_secret = query.split('&').find_map(|p| p.strip_prefix("s=")).unwrap();
    hex::decode(hex_secret).unwrap()
}

fn server_addr(offer: &PairingOffer) -> SocketAddr {
    offer.address.parse().unwrap()
}

async fn ping(addr: SocketAddr, fingerprint: [u8; 32], token: &str) -> Option<Value> {
    let mut phone = Phone::connect(addr, Some(fingerprint)).await?;
    phone.send(&json!({"v": PROTOCOL, "token": token})).await;
    phone.recv().await?;
    phone.send(&json!({"cmd": "ping"})).await;
    phone.recv().await
}

#[tokio::test]
async fn a_paired_phone_gets_an_answer_and_a_stranger_gets_none() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    let addr = server_addr(&offer);
    let fingerprint = fp_bytes(&offer.fingerprint);
    let token = pair(addr, &offer).await;
    assert_eq!(endpoint.paired_count(), 1);

    let answer = ping(addr, fingerprint, &token).await.unwrap();
    assert_eq!(answer["ok"], true);
    assert_eq!(answer["agent"], "construct-agent");

    // A wrong token, a token-shaped guess, no known fields and text that is not JSON: all silent.
    let wrong = "0".repeat(64);
    for hello in [
        json!({"v": PROTOCOL, "token": wrong}),
        json!({"v": PROTOCOL}),
        json!({"v": PROTOCOL, "token": 5}),
        json!({"token": token}),
    ] {
        let mut stranger = Phone::connect(addr, Some(fingerprint)).await.unwrap();
        stranger.send(&hello).await;
        assert_eq!(stranger.recv().await, None);
    }
    let mut stranger = Phone::connect(addr, Some(fingerprint)).await.unwrap();
    stranger
        .writer
        .write_all(b"GET / HTTP/1.1\r\n\r\n")
        .await
        .unwrap();
    assert_eq!(stranger.recv().await, None);
}

#[tokio::test]
async fn a_secret_works_once() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    let addr = server_addr(&offer);
    pair(addr, &offer).await;

    // The same secret again, and the short code of the same pairing, are silent.
    let mut again = Phone::connect(addr, Some(fp_bytes(&offer.fingerprint)))
        .await
        .unwrap();
    let seen = again.seen;
    again
        .send(&pair_message(&qr_secret(&offer), &seen, PROTOCOL, "Second"))
        .await;
    assert_eq!(again.recv().await, None);
    let mut by_code = Phone::connect(addr, Some(fp_bytes(&offer.fingerprint)))
        .await
        .unwrap();
    by_code
        .send(&pair_message(
            offer.code.as_bytes(),
            &seen,
            PROTOCOL,
            "Third",
        ))
        .await;
    assert_eq!(by_code.recv().await, None);
    assert_eq!(endpoint.paired_count(), 1);
}

#[tokio::test]
async fn the_short_code_pairs_and_the_phone_checks_the_agent() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    assert_eq!(offer.code.len(), 8);
    assert!(offer.code.chars().all(|c| c.is_ascii_digit()));

    // Typing a code gives the phone no fingerprint: it takes the certificate it sees and the
    // proofs bind to it.
    let mut phone = Phone::connect(server_addr(&offer), None).await.unwrap();
    let seen = phone.seen;
    phone
        .send(&pair_message(
            offer.code.as_bytes(),
            &seen,
            PROTOCOL,
            "Typed",
        ))
        .await;
    let reply = phone.recv().await.unwrap();
    assert_eq!(reply["paired"], true);
    assert_eq!(
        reply["agent_proof"],
        proof(offer.code.as_bytes(), Role::Agent, &seen)
    );
    assert_eq!(seen, fp_bytes(&offer.fingerprint));
}

#[tokio::test]
async fn a_proof_for_another_certificate_is_refused_and_three_wrong_proofs_close_the_pairing() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    let addr = server_addr(&offer);
    let fingerprint = fp_bytes(&offer.fingerprint);

    // A computer in the middle has another certificate, so the proof it relays binds to that one.
    let mut phone = Phone::connect(addr, Some(fingerprint)).await.unwrap();
    phone
        .send(&pair_message(
            &qr_secret(&offer),
            &[7u8; 32],
            PROTOCOL,
            "Relayed",
        ))
        .await;
    assert_eq!(phone.recv().await, None);
    for _ in 0..2 {
        let mut guess = Phone::connect(addr, Some(fingerprint)).await.unwrap();
        guess
            .send(&pair_message(b"00000000", &fingerprint, PROTOCOL, "Guess"))
            .await;
        assert_eq!(guess.recv().await, None);
    }
    // Three failures are used: even the right secret is silent now.
    let mut late = Phone::connect(addr, Some(fingerprint)).await.unwrap();
    late.send(&pair_message(
        &qr_secret(&offer),
        &fingerprint,
        PROTOCOL,
        "Late",
    ))
    .await;
    assert_eq!(late.recv().await, None);
    assert_eq!(endpoint.paired_count(), 0);
}

#[tokio::test]
async fn a_pairing_that_ran_out_of_time_or_was_closed_is_silent() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let fingerprint;
    let addr;
    let secret;
    {
        let offer = endpoint.open_pairing(LOCAL, Duration::ZERO).unwrap();
        addr = server_addr(&offer);
        fingerprint = fp_bytes(&offer.fingerprint);
        secret = qr_secret(&offer);
    }
    let mut phone = Phone::connect(addr, Some(fingerprint)).await.unwrap();
    phone
        .send(&pair_message(&secret, &fingerprint, PROTOCOL, "Late"))
        .await;
    assert_eq!(phone.recv().await, None);

    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    endpoint.close_pairing();
    let mut phone = Phone::connect(addr, Some(fingerprint)).await.unwrap();
    phone
        .send(&pair_message(
            &qr_secret(&offer),
            &fingerprint,
            PROTOCOL,
            "Closed",
        ))
        .await;
    assert_eq!(phone.recv().await, None);
}

#[tokio::test]
async fn a_phone_that_is_too_old_is_told_to_update_and_only_when_it_proved_itself() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    let addr = server_addr(&offer);
    let fingerprint = fp_bytes(&offer.fingerprint);

    // An old phone with no proof gets nothing.
    let mut stranger = Phone::connect(addr, Some(fingerprint)).await.unwrap();
    stranger
        .send(&json!({"v": 0, "token": "0".repeat(64)}))
        .await;
    assert_eq!(stranger.recv().await, None);

    // An old phone with the right secret is told, and the secret is not used up.
    let mut old = Phone::connect(addr, Some(fingerprint)).await.unwrap();
    old.send(&pair_message(&qr_secret(&offer), &fingerprint, 0, "Old"))
        .await;
    let reply = old.recv().await.unwrap();
    assert_eq!(reply["error"], "update needed");
    assert_eq!(reply["min_version"], 1);
    assert_eq!(endpoint.paired_count(), 0);
    let token = pair(addr, &offer).await;

    // A paired phone that went old is told as well; one from the future too, in other words.
    let mut old = Phone::connect(addr, Some(fingerprint)).await.unwrap();
    old.send(&json!({"v": 0, "token": token})).await;
    assert_eq!(old.recv().await.unwrap()["error"], "update needed");
    let mut newer = Phone::connect(addr, Some(fingerprint)).await.unwrap();
    newer
        .send(&json!({"v": PROTOCOL + 1, "token": token}))
        .await;
    assert_eq!(newer.recv().await.unwrap()["error"], "agent update needed");
}

/// Copy `from` to `to` and keep a copy of every byte.
async fn pump(
    mut from: tokio::net::tcp::OwnedReadHalf,
    mut to: tokio::net::tcp::OwnedWriteHalf,
    seen: Arc<Mutex<Vec<u8>>>,
) {
    let mut buf = vec![0u8; 4096];
    loop {
        let n = from.read(&mut buf).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        seen.lock().unwrap().extend_from_slice(&buf[..n]);
        if to.write_all(&buf[..n]).await.is_err() {
            break;
        }
    }
}

/// A listener between a phone and the endpoint that copies every byte, both ways, and keeps a copy.
async fn capture(target: SocketAddr) -> (SocketAddr, Arc<Mutex<Vec<u8>>>) {
    let listener = TcpListener::bind(SocketAddr::new(LOCAL, 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&seen);
    tokio::spawn(async move {
        loop {
            let (client, _) = listener.accept().await.unwrap();
            let server = TcpStream::connect(target).await.unwrap();
            let (client_read, client_write) = client.into_split();
            let (server_read, server_write) = server.into_split();
            tokio::spawn(pump(client_read, server_write, Arc::clone(&kept)));
            tokio::spawn(pump(server_read, client_write, Arc::clone(&kept)));
        }
    });
    (addr, seen)
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[tokio::test]
async fn the_bytes_on_the_wire_hold_nothing_readable() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    let (via, wire) = capture(server_addr(&offer)).await;
    let fingerprint = fp_bytes(&offer.fingerprint);

    let mut phone = Phone::connect(via, Some(fingerprint)).await.unwrap();
    phone
        .send(&pair_message(
            &qr_secret(&offer),
            &fingerprint,
            PROTOCOL,
            "Wire Marker Phone 7731",
        ))
        .await;
    let reply = phone.recv().await.unwrap();
    let token = reply["token"].as_str().unwrap().to_owned();
    drop(phone);
    let answer = ping(via, fingerprint, &token).await.unwrap();
    assert_eq!(answer["ok"], true);

    let wire = wire.lock().unwrap();
    assert!(wire.len() > 100, "the capture must hold the traffic");
    for text in [
        "Wire Marker Phone 7731",
        "pair",
        "proof",
        "token",
        &token,
        &offer.fingerprint,
        "construct-agent",
        "cmd",
        "ping",
        "agent_version",
    ] {
        assert!(!contains(&wire, text), "the wire shows {text:?}");
    }
}

#[tokio::test]
async fn nothing_listens_in_plain_text() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    let mut raw = TcpStream::connect(server_addr(&offer)).await.unwrap();
    raw.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let mut got = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), raw.read_to_end(&mut got)).await;
    assert!(!contains(&got, "HTTP"));
    assert!(!contains(&got, "construct"));
}

#[tokio::test]
async fn a_line_that_is_too_long_is_silent() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    let addr = server_addr(&offer);
    let fingerprint = fp_bytes(&offer.fingerprint);
    let token = pair(addr, &offer).await;
    // A line with a right token is answered; the same line padded over the limit is not.
    let mut phone = Phone::connect(addr, Some(fingerprint)).await.unwrap();
    phone.send(&json!({"v": PROTOCOL, "token": token})).await;
    assert_eq!(phone.recv().await.unwrap()["ok"], true);
    let mut phone = Phone::connect(addr, Some(fingerprint)).await.unwrap();
    let pad = "a".repeat(construct_agent::phone::MAX_LINE);
    phone
        .send(&json!({"v": PROTOCOL, "token": token, "pad": pad}))
        .await;
    assert_eq!(phone.recv().await, None);
}

#[tokio::test]
async fn the_payload_matches_the_certificate_the_handshake_shows() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let host: IpAddr = "172.20.10.2".parse().unwrap();
    let offer = endpoint.open_pairing(host, TTL).unwrap();
    let port = endpoint.local_addr().port();
    assert_eq!(offer.address, format!("172.20.10.2:{port}"));
    let query = offer
        .payload
        .strip_prefix("construct-pair://172.20.10.2:")
        .unwrap();
    let (payload_port, query) = query.split_once('?').unwrap();
    assert_eq!(payload_port, port.to_string());
    let fields: Vec<&str> = query.split('&').collect();
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0], format!("v={PROTOCOL}"));
    // What the phone would pin is what the handshake shows.
    let phone = Phone::connect(SocketAddr::new(LOCAL, port), None)
        .await
        .unwrap();
    assert_eq!(fields[1], format!("fp={}", hex::encode(phone.seen)));
    assert_eq!(fields[2].len(), 2 + 64);
    assert!(fields[2].starts_with("s="));
}

#[test]
fn the_proofs_match_fixed_values_and_the_roles_differ() {
    // Values made apart from this code with Python's hmac and hashlib: HMAC-SHA-256 over
    // "construct-pair", a zero byte, the role and the fingerprint, keyed with the secret.
    let fingerprint: [u8; 32] = std::array::from_fn(|i| u8::try_from(i).unwrap());
    let secret: Vec<u8> = (100u8..132).collect();
    assert_eq!(
        proof(&secret, Role::Phone, &fingerprint),
        "24d3b6880809f7fa38564b20d0442b7ae18ca931a8537eaccea4becc7ed751b5"
    );
    assert_eq!(
        proof(&secret, Role::Agent, &fingerprint),
        "3645785c711cf914b4148588aa38471121bb4e332d8de0bbd21c9940e56aaaf5"
    );
    assert_eq!(
        proof(b"12345678", Role::Phone, &fingerprint),
        "e796d48586c680dfbd473309ad46c67498734e9488d8185029845ee28a9842f8"
    );
    assert_eq!(
        proof(b"12345678", Role::Agent, &fingerprint),
        "d0d5bf0ca45e43b037c10fb54993dfd3492e92c32881423173b6510dcc5d1885"
    );
}

#[tokio::test]
async fn two_wrong_proofs_leave_the_pairing_open() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    let addr = server_addr(&offer);
    let fingerprint = fp_bytes(&offer.fingerprint);
    for _ in 0..2 {
        let mut guess = Phone::connect(addr, Some(fingerprint)).await.unwrap();
        guess
            .send(&pair_message(b"00000000", &fingerprint, PROTOCOL, "Guess"))
            .await;
        assert_eq!(guess.recv().await, None);
    }
    pair(addr, &offer).await;
    assert_eq!(endpoint.paired_count(), 1);
}

#[tokio::test]
async fn an_agent_that_is_older_than_the_phone_says_so_and_keeps_the_secret() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    let addr = server_addr(&offer);
    let fingerprint = fp_bytes(&offer.fingerprint);
    let mut newer = Phone::connect(addr, Some(fingerprint)).await.unwrap();
    newer
        .send(&pair_message(
            &qr_secret(&offer),
            &fingerprint,
            PROTOCOL + 1,
            "Newer",
        ))
        .await;
    assert_eq!(newer.recv().await.unwrap()["error"], "agent update needed");
    assert_eq!(endpoint.paired_count(), 0);
    pair(addr, &offer).await;
}

#[tokio::test]
async fn when_the_phone_cannot_be_saved_the_secret_is_spent_and_the_phone_gets_nothing() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    let addr = server_addr(&offer);
    let fingerprint = fp_bytes(&offer.fingerprint);
    // A directory where the file belongs makes every write fail.
    std::fs::create_dir(dir.0.join("phones.json")).unwrap();
    let mut phone = Phone::connect(addr, Some(fingerprint)).await.unwrap();
    phone
        .send(&pair_message(
            &qr_secret(&offer),
            &fingerprint,
            PROTOCOL,
            "Unsaved",
        ))
        .await;
    assert_eq!(phone.recv().await, None);
    assert_eq!(endpoint.paired_count(), 0);
    let mut again = Phone::connect(addr, Some(fingerprint)).await.unwrap();
    again
        .send(&pair_message(
            &qr_secret(&offer),
            &fingerprint,
            PROTOCOL,
            "Again",
        ))
        .await;
    assert_eq!(again.recv().await, None);
}

#[tokio::test]
async fn a_phones_file_of_a_newer_format_is_not_started_over() {
    let dir = Dir::new();
    std::fs::create_dir_all(&dir.0).unwrap();
    std::fs::write(dir.0.join("phones.json"), br#"{"version":2,"phones":[]}"#).unwrap();
    let started = Endpoint::start(Config {
        dir: dir.0.clone(),
        bind: SocketAddr::new(LOCAL, 0),
    })
    .await;
    assert!(started.is_err());
    assert_eq!(
        std::fs::read(dir.0.join("phones.json")).unwrap(),
        br#"{"version":2,"phones":[]}"#
    );
}

#[tokio::test]
async fn paired_phones_and_the_certificate_survive_a_restart_and_secrets_stay_off_disk() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    let token = pair(server_addr(&offer), &offer).await;
    let fingerprint = fp_bytes(&offer.fingerprint);
    endpoint.stop().await;

    let endpoint = start(&dir).await;
    let again = endpoint.open_pairing(LOCAL, TTL).unwrap();
    assert_eq!(again.fingerprint, offer.fingerprint);
    assert_eq!(endpoint.paired_count(), 1);
    let answer = ping(server_addr(&again), fingerprint, &token)
        .await
        .unwrap();
    assert_eq!(answer["ok"], true);

    let phones = std::fs::read_to_string(dir.0.join("phones.json")).unwrap();
    assert!(!phones.contains(&token));
    assert!(!phones.contains(&offer.code));
    for file in ["phones.json", "phone-key.der", "phone-cert.der"] {
        let mode = std::fs::metadata(dir.0.join(file))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "{file}");
    }
}

/// A port nothing listens on now, for a service that must come back on the same port.
fn free_port() -> u16 {
    std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn service(dir: &Dir, port: u16) -> Service {
    Service::start(dir.0.clone(), SocketAddr::new(LOCAL, port), Some(LOCAL))
        .await
        .unwrap()
}

/// Wait, up to five seconds, until the port is closed; whether it closed is the answer.
async fn closes(port: u16) -> bool {
    for _ in 0..100 {
        if !is_open(port).await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

async fn is_open(port: u16) -> bool {
    TcpStream::connect(SocketAddr::new(LOCAL, port))
        .await
        .is_ok()
}

#[tokio::test]
async fn no_port_is_open_on_a_fresh_start_and_a_pairing_opens_it() {
    let dir = Dir::new();
    let port = free_port();
    let phones = service(&dir, port).await;
    assert!(!is_open(port).await);

    let offer = phones.open_pairing(TTL).await.unwrap();
    assert!(is_open(port).await);
    let token = pair_named(server_addr(&offer), &offer, "Anna's phone").await;

    // The pairing is used up, but a phone is paired now: the port stays open.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(is_open(port).await);
    let fingerprint = fp_bytes(&offer.fingerprint);
    assert_eq!(
        ping(server_addr(&offer), fingerprint, &token)
            .await
            .unwrap()["ok"],
        true
    );
    let list = phones.phones();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "Anna's phone");
    assert!(list[0].paired_at > 0);
}

#[tokio::test]
async fn a_pairing_that_is_closed_or_runs_out_leaves_no_port_when_no_phone_paired() {
    let dir = Dir::new();
    let port = free_port();
    let phones = service(&dir, port).await;

    phones.open_pairing(TTL).await.unwrap();
    assert!(is_open(port).await);
    phones.close_pairing().await;
    assert!(closes(port).await);

    phones.open_pairing(Duration::from_secs(1)).await.unwrap();
    assert!(is_open(port).await);
    assert!(closes(port).await);

    // Three wrong proofs end the pairing, and the port goes with it.
    let offer = phones.open_pairing(TTL).await.unwrap();
    let fingerprint = fp_bytes(&offer.fingerprint);
    for _ in 0..3 {
        let mut phone = Phone::connect(server_addr(&offer), Some(fingerprint))
            .await
            .unwrap();
        let seen = phone.seen;
        phone
            .send(&pair_message(b"wrong", &seen, PROTOCOL, "Intruder"))
            .await;
        assert_eq!(phone.recv().await, None);
    }
    assert!(closes(port).await);
    assert!(phones.phones().is_empty());
}

#[tokio::test]
async fn after_a_pairing_and_a_restart_the_agent_answers_the_phone() {
    let dir = Dir::new();
    let port = free_port();
    let phones = service(&dir, port).await;
    let offer = phones.open_pairing(TTL).await.unwrap();
    let token = pair(server_addr(&offer), &offer).await;
    let fingerprint = fp_bytes(&offer.fingerprint);
    phones.stop().await;
    assert!(!is_open(port).await);

    // A new service over the same files listens at once, with no pairing open.
    let phones = service(&dir, port).await;
    assert!(is_open(port).await);
    let addr = SocketAddr::new(LOCAL, port);
    assert_eq!(ping(addr, fingerprint, &token).await.unwrap()["ok"], true);
    assert_eq!(phones.phones().len(), 1);
}

#[tokio::test]
async fn two_phones_pair_and_unpairing_one_leaves_the_other() {
    let dir = Dir::new();
    let port = free_port();
    let phones = service(&dir, port).await;
    let first = phones.open_pairing(TTL).await.unwrap();
    let addr = server_addr(&first);
    let fingerprint = fp_bytes(&first.fingerprint);
    let token_a = pair_named(addr, &first, "First").await;
    let second = phones.open_pairing(TTL).await.unwrap();
    let token_b = pair_named(addr, &second, "Second").await;

    let list = phones.phones();
    assert_eq!(
        list.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["First", "Second"]
    );
    assert_ne!(list[0].id, list[1].id);

    assert!(phones.unpair(&list[0].id).await.unwrap());
    assert!(!phones.unpair(&list[0].id).await.unwrap());
    assert!(ping(addr, fingerprint, &token_a).await.is_none());
    assert_eq!(ping(addr, fingerprint, &token_b).await.unwrap()["ok"], true);
    assert!(is_open(port).await);
}

#[tokio::test]
async fn the_last_unpair_closes_the_port_and_drops_an_open_channel() {
    let dir = Dir::new();
    let port = free_port();
    let phones = service(&dir, port).await;
    let offer = phones.open_pairing(TTL).await.unwrap();
    let addr = server_addr(&offer);
    let fingerprint = fp_bytes(&offer.fingerprint);
    let token = pair(addr, &offer).await;

    // A channel that is open and has been answered.
    let mut phone = Phone::connect(addr, Some(fingerprint)).await.unwrap();
    phone.send(&json!({"v": PROTOCOL, "token": token})).await;
    assert_eq!(phone.recv().await.unwrap()["ok"], true);
    phone.send(&json!({"cmd": "ping"})).await;
    assert_eq!(phone.recv().await.unwrap()["ok"], true);

    let id = phones.phones()[0].id.clone();
    assert!(phones.unpair(&id).await.unwrap());

    // The channel is closed at once: the agent ends it before any further command. Then a
    // command gets no answer either.
    assert_eq!(phone.recv().await, None);
    phone.send(&json!({"cmd": "ping"})).await;
    assert_eq!(phone.recv().await, None);
    assert!(closes(port).await);
    assert!(phones.phones().is_empty());
    assert!(!phones.unpair(&id).await.unwrap());
}

#[tokio::test]
async fn an_unpaired_phone_gets_no_answer_while_the_port_is_open_for_another_pairing() {
    let dir = Dir::new();
    let port = free_port();
    let phones = service(&dir, port).await;
    let offer = phones.open_pairing(TTL).await.unwrap();
    let addr = server_addr(&offer);
    let fingerprint = fp_bytes(&offer.fingerprint);
    let token = pair(addr, &offer).await;
    phones.open_pairing(TTL).await.unwrap();

    let id = phones.phones()[0].id.clone();
    phones.unpair(&id).await.unwrap();
    // A pairing is open, so the port is; the old token still gets nothing.
    assert!(is_open(port).await);
    assert!(ping(addr, fingerprint, &token).await.is_none());
}

fn mode_of(path: &std::path::Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[tokio::test]
async fn a_file_with_no_phones_starts_and_one_this_agent_does_not_know_is_refused() {
    let dir = Dir::new();
    std::fs::create_dir_all(&dir.0).unwrap();
    let file = dir.0.join("phones.json");
    let port = free_port();

    std::fs::write(&file, r#"{"version":1}"#).unwrap();
    let phones = service(&dir, port).await;
    assert!(phones.phones().is_empty());
    assert!(!is_open(port).await);

    for unknown in [
        r#"{"version":2,"phones":[]}"#,
        r#"{"version":1,"phones":[],"later":true}"#,
        r#"{"version":1,"phones":[{"id":"a","name":"x","token_hash":"b","paired_at":1,"later":1}]}"#,
    ] {
        std::fs::write(&file, unknown).unwrap();
        let started =
            Service::start(dir.0.clone(), SocketAddr::new(LOCAL, port), Some(LOCAL)).await;
        assert!(started.is_err(), "{unknown}");
        // The file is left as it was, not rewritten without what the agent did not know.
        assert_eq!(std::fs::read_to_string(&file).unwrap(), unknown);
    }
}

#[tokio::test]
async fn an_unpaired_phone_stays_unpaired_after_a_restart_and_the_file_stays_user_only() {
    let dir = Dir::new();
    let port = free_port();
    let phones = service(&dir, port).await;
    let first = phones.open_pairing(TTL).await.unwrap();
    let addr = server_addr(&first);
    let fingerprint = fp_bytes(&first.fingerprint);
    let token_a = pair_named(addr, &first, "First").await;
    let second = phones.open_pairing(TTL).await.unwrap();
    let token_b = pair_named(addr, &second, "Second").await;

    // The file is user-only whatever the umask: loosen it, and a write must tighten it again.
    let file = dir.0.join("phones.json");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let id = phones.phones()[0].id.clone();
    assert!(phones.unpair(&id).await.unwrap());
    assert_eq!(mode_of(&file), 0o600);
    phones.stop().await;

    let phones = service(&dir, port).await;
    assert_eq!(phones.phones().len(), 1);
    assert!(ping(addr, fingerprint, &token_a).await.is_none());
    assert_eq!(ping(addr, fingerprint, &token_b).await.unwrap()["ok"], true);

    // The last one: after a restart there is no port at all.
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let id = phones.phones()[0].id.clone();
    assert!(phones.unpair(&id).await.unwrap());
    assert_eq!(mode_of(&file), 0o600);
    phones.stop().await;
    let phones = service(&dir, port).await;
    assert!(phones.phones().is_empty());
    assert!(!is_open(port).await);
}

#[tokio::test]
async fn phones_can_be_listed_and_unpaired_when_their_port_is_taken() {
    let dir = Dir::new();
    let port = free_port();
    let phones = service(&dir, port).await;
    let offer = phones.open_pairing(TTL).await.unwrap();
    pair(server_addr(&offer), &offer).await;
    phones.stop().await;

    let holder = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).unwrap();
    let phones = service(&dir, port).await;
    let id = phones.phones()[0].id.clone();
    assert!(phones.unpair(&id).await.unwrap());
    drop(holder);
    assert!(phones.phones().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_app_commands_open_close_list_and_unpair_and_show_no_secret() {
    let dir = Dir::new();
    let port = free_port();
    let phones = service(&dir, port).await;
    let ask = |command: &'static str, argument: String| {
        let phones = phones.clone();
        async move {
            let line = tokio::task::spawn_blocking(move || phones.command(command, &argument))
                .await
                .unwrap();
            (serde_json::from_str::<Value>(&line).unwrap(), line)
        }
    };

    let (opened, _) = ask("phone-pairing-open", String::new()).await;
    assert!(
        opened["payload"]
            .as_str()
            .unwrap()
            .starts_with("construct-pair://")
    );
    assert_eq!(opened["code"].as_str().unwrap().len(), 8);
    assert!(is_open(port).await);

    let offer = PairingOffer {
        address: opened["address"].as_str().unwrap().to_owned(),
        payload: opened["payload"].as_str().unwrap().to_owned(),
        code: opened["code"].as_str().unwrap().to_owned(),
        fingerprint: opened["payload"]
            .as_str()
            .unwrap()
            .split('&')
            .find_map(|p| p.strip_prefix("fp="))
            .unwrap()
            .to_owned(),
    };
    let token = pair_named(server_addr(&offer), &offer, "Anna's phone").await;

    let (listed, line) = ask("phone-list", String::new()).await;
    assert_eq!(listed["phones"][0]["name"], "Anna's phone");
    assert!(listed["phones"][0]["paired_at"].as_u64().unwrap() > 0);
    assert_eq!(listed["phones"][0].as_object().unwrap().len(), 3);
    assert!(!line.contains(&token));
    assert!(!line.contains(&offer.code));

    let (closed, _) = ask("phone-pairing-close", String::new()).await;
    assert_eq!(closed["ok"], true);
    assert!(is_open(port).await);

    let (error, _) = ask("phone-unpair", r#"{"id":"nobody"}"#.to_owned()).await;
    assert_eq!(error["error"], "no such phone");
    let (error, _) = ask("phone-unpair", "not json".to_owned()).await;
    assert_eq!(error["error"], "invalid phone request");

    let id = listed["phones"][0]["id"].as_str().unwrap();
    let (after, _) = ask("phone-unpair", json!({"id": id}).to_string()).await;
    assert_eq!(after["phones"], json!([]));
    assert!(!is_open(port).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_local_socket_serves_the_phone_commands_and_answers_an_error_without_them() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    fn ask(path: &std::path::Path, line: &str) -> Value {
        let mut stream = UnixStream::connect(path).unwrap();
        stream.write_all(format!("{line}\n").as_bytes()).unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        serde_json::from_str(&reply).unwrap()
    }

    let dir = Dir::new();
    let with = dir.0.join("with").join("agent.sock");
    let without = dir.0.join("without").join("agent.sock");
    let phones = service(&dir, free_port()).await;
    let server = construct_agent::ipc::Server::bind(&with)
        .unwrap()
        .with_phones(phones);
    std::thread::spawn(move || server.serve());
    let bare = construct_agent::ipc::Server::bind(&without).unwrap();
    std::thread::spawn(move || bare.serve());

    tokio::task::spawn_blocking(move || {
        assert_eq!(ask(&with, "phone-list"), json!({"phones": []}));
        assert!(ask(&with, "phone-pairing-open")["code"].is_string());
        assert_eq!(ask(&with, "phone-pairing-close"), json!({"ok": true}));
        assert_eq!(
            ask(&without, "phone-list"),
            json!({"error": "phones are not available"})
        );
    })
    .await
    .unwrap();
}
