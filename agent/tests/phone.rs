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

use construct_agent::phone::{Config, Endpoint, PROTOCOL, PairingOffer, Service, fingerprint_of};
use phone_pairing::{BINDING_LEN, CodeRequest, EXPORTER_LABEL, Role, qr_proof};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme, version};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf};
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
    binding: [u8; BINDING_LEN],
    typed: bool,
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
        let mut config = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&version::TLS13])
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::clone(&pin) as Arc<dyn ServerCertVerifier>)
            .with_no_client_auth();
        if pinned.is_none() {
            config.alpn_protocols = vec![phone_pairing::TYPED_CAPABILITY.to_vec()];
        }
        let stream = TcpStream::connect(addr).await.ok()?;
        let tls = TlsConnector::from(Arc::new(config))
            .connect(ServerName::IpAddress(addr.ip().into()), stream)
            .await
            .ok()?;
        let seen = (*pin.seen.lock().unwrap())?;
        let typed = tls.get_ref().1.alpn_protocol() == Some(phone_pairing::TYPED_CAPABILITY);
        let binding = tls
            .get_ref()
            .1
            .export_keying_material([0; BINDING_LEN], EXPORTER_LABEL, None)
            .ok()?;
        let (reader, writer) = tokio::io::split(tls);
        Some(Self {
            reader: BufReader::new(reader),
            writer,
            seen,
            binding,
            typed,
        })
    }

    async fn start_code(&mut self, code: &str, version: u64) -> (CodeRequest, Option<Vec<u8>>) {
        assert!(self.typed);
        let (code, request) = CodeRequest::new(code.as_bytes()).unwrap();
        self.send(&json!({"v": version, "pair_code": {"request": hex::encode(request)}}))
            .await;
        let reply = self.recv().await.unwrap();
        let response = reply["challenge"]
            .as_str()
            .map(|challenge| hex::decode(challenge).unwrap());
        if response.is_none() {
            // An exhausted, used or expired offer replies explicitly on the typed protocol.
            // No such reply is available on the QR route.
            assert_eq!(reply["error"], phone_pairing::CODE_REJECTED);
        }
        (code, response)
    }

    async fn finish_code(&mut self, proof: &[u8]) -> Option<Value> {
        self.send(&json!({"confirmation": hex::encode(proof), "name": "Typed"}))
            .await;
        self.recv().await
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
        let read = tokio::time::timeout(Duration::from_secs(5), self.reader.read_line(&mut line))
            .await
            .unwrap_or_else(|_| panic!("the agent neither answered nor closed"));
        match read {
            Ok(n) if n > 0 => Some(serde_json::from_str(&line).unwrap()),
            _ => None,
        }
    }
}

fn proof(secret: &[u8; 32], role: Role, seen: &[u8; 32]) -> String {
    hex::encode(qr_proof(secret, role, seen).as_ref())
}

fn pair_message(secret: &[u8; 32], seen: &[u8; 32], version: u64, name: &str) -> Value {
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

fn qr_secret(offer: &PairingOffer) -> [u8; 32] {
    let query = offer.payload.split_once('?').unwrap().1;
    let hex_secret = query.split('&').find_map(|p| p.strip_prefix("s=")).unwrap();
    fp_bytes(hex_secret)
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

    // The repeated QR proof stays silent, and the typed route explicitly rejects reuse.
    let mut again = Phone::connect(addr, Some(fp_bytes(&offer.fingerprint)))
        .await
        .unwrap();
    let seen = again.seen;
    again
        .send(&pair_message(&qr_secret(&offer), &seen, PROTOCOL, "Second"))
        .await;
    assert_eq!(again.recv().await, None);
    let mut by_code = Phone::connect(addr, None).await.unwrap();
    assert!(by_code.start_code(&offer.code, PROTOCOL).await.1.is_none());
    assert_eq!(endpoint.paired_count(), 1);
}

#[tokio::test]
async fn a_code_request_requires_the_negotiated_tls_protocol() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    let mut phone = Phone::connect(server_addr(&offer), Some(fp_bytes(&offer.fingerprint)))
        .await
        .unwrap();
    assert!(!phone.typed);
    let (_, request) = CodeRequest::new(offer.code.as_bytes()).unwrap();
    phone
        .send(&json!({"v": PROTOCOL, "pair_code": {"request": hex::encode(request)}}))
        .await;
    assert!(phone.recv().await.is_none());
    assert!(endpoint.pairing_is_open());
}

#[tokio::test]
async fn the_short_code_pairs_and_the_phone_checks_the_agent() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    assert_eq!(offer.code.len(), 8);
    assert!(offer.code.chars().all(|c| c.is_ascii_digit()));

    // The code authenticates the computer on the negotiated TLS channel before it is pinned.
    let mut phone = Phone::connect(server_addr(&offer), None).await.unwrap();
    let (code, challenge) = phone.start_code(&offer.code, PROTOCOL).await;
    let confirmation = code
        .confirm(
            offer.code.as_bytes(),
            phone_pairing::CodeChallenge::parse(&challenge.unwrap()).unwrap(),
            &phone.binding,
            PROTOCOL,
        )
        .unwrap();
    let reply = phone.finish_code(&confirmation).await.unwrap();
    assert_eq!(reply["paired"], true);
    let token = reply["token"].as_str().unwrap();
    assert_eq!(phone.seen, fp_bytes(&offer.fingerprint));
    assert_eq!(
        ping(server_addr(&offer), phone.seen, token).await.unwrap()["ok"],
        true
    );
}

#[tokio::test]
async fn a_confirmation_can_consume_the_code_even_if_the_phone_does_not_read_the_reply() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    let addr = server_addr(&offer);
    let mut phone = Phone::connect(addr, None).await.unwrap();
    let (code, challenge) = phone.start_code(&offer.code, PROTOCOL).await;
    let proof = code
        .confirm(
            offer.code.as_bytes(),
            phone_pairing::CodeChallenge::parse(&challenge.unwrap()).unwrap(),
            &phone.binding,
            PROTOCOL,
        )
        .unwrap();
    phone
        .send(&json!({"confirmation": hex::encode(proof), "name": "Typed"}))
        .await;
    tokio::time::timeout(Duration::from_secs(5), endpoint.wait_pairing_over())
        .await
        .unwrap();
    drop(phone);
    assert_eq!(endpoint.paired_count(), 1);

    let mut retry = Phone::connect(addr, None).await.unwrap();
    assert!(retry.start_code(&offer.code, PROTOCOL).await.1.is_none());
    let expired = endpoint.open_pairing(LOCAL, Duration::ZERO).unwrap();
    let mut retry = Phone::connect(addr, None).await.unwrap();
    assert!(retry.start_code(&expired.code, PROTOCOL).await.1.is_none());
    assert_eq!(endpoint.paired_count(), 1);
}

#[tokio::test]
async fn abandoned_code_challenges_count_toward_the_offer_limit() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    let addr = server_addr(&offer);
    let mut phones = Vec::new();
    for _ in 0..3 {
        let mut phone = Phone::connect(addr, None).await.unwrap();
        assert!(phone.start_code(&offer.code, PROTOCOL).await.1.is_some());
        phones.push(phone);
    }
    let mut later = Phone::connect(addr, None).await.unwrap();
    assert!(later.start_code(&offer.code, PROTOCOL).await.1.is_none());
    drop(phones);
    tokio::time::timeout(Duration::from_secs(5), endpoint.wait_pairing_over())
        .await
        .unwrap();
    assert!(!endpoint.pairing_is_open());
    assert_eq!(endpoint.paired_count(), 0);
}

#[tokio::test]
async fn a_phone_that_is_too_old_is_told_to_update_by_typed_code_and_the_offer_stays_open() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    let addr = server_addr(&offer);

    // Both too old and too new prove the code first, and are told only then.
    for (version, error) in [(0, "update needed"), (PROTOCOL + 1, "agent update needed")] {
        let mut phone = Phone::connect(addr, None).await.unwrap();
        let (code, challenge) = phone.start_code(&offer.code, version).await;
        let confirmation = code
            .confirm(
                offer.code.as_bytes(),
                phone_pairing::CodeChallenge::parse(&challenge.unwrap()).unwrap(),
                &phone.binding,
                version,
            )
            .unwrap();
        let reply = phone.finish_code(&confirmation).await.unwrap();
        assert_eq!(reply["error"], error);
        assert!(reply.get("token").is_none());
        assert_eq!(endpoint.paired_count(), 0);
        assert!(endpoint.pairing_is_open());
    }

    // The code is not used up: a phone of the right version still pairs with it.
    let mut phone = Phone::connect(addr, None).await.unwrap();
    let (code, challenge) = phone.start_code(&offer.code, PROTOCOL).await;
    let confirmation = code
        .confirm(
            offer.code.as_bytes(),
            phone_pairing::CodeChallenge::parse(&challenge.unwrap()).unwrap(),
            &phone.binding,
            PROTOCOL,
        )
        .unwrap();
    assert_eq!(
        phone.finish_code(&confirmation).await.unwrap()["paired"],
        true
    );
}

#[tokio::test]
async fn wrong_qr_proofs_and_abandoned_code_challenges_share_the_limit_of_three() {
    let dir = Dir::new();
    let endpoint = start(&dir).await;
    let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
    let addr = server_addr(&offer);
    let fingerprint = fp_bytes(&offer.fingerprint);

    // One wrong QR proof uses one attempt.
    let mut guess = Phone::connect(addr, Some(fingerprint)).await.unwrap();
    guess
        .send(&pair_message(&[0; 32], &fingerprint, PROTOCOL, "Guess"))
        .await;
    assert_eq!(guess.recv().await, None);

    // Two challenges that wait for their confirmation hold the other two.
    let mut waiting = Vec::new();
    for _ in 0..2 {
        let mut phone = Phone::connect(addr, None).await.unwrap();
        assert!(phone.start_code(&offer.code, PROTOCOL).await.1.is_some());
        waiting.push(phone);
    }
    let mut refused = Phone::connect(addr, None).await.unwrap();
    assert!(refused.start_code(&offer.code, PROTOCOL).await.1.is_none());
    assert!(endpoint.pairing_is_open());

    // When the two end without a confirmation the budget is used up, and even the right
    // QR secret is silent.
    drop(waiting);
    tokio::time::timeout(Duration::from_secs(5), endpoint.wait_pairing_over())
        .await
        .unwrap();
    assert!(!endpoint.pairing_is_open());
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
            .send(&pair_message(&[0; 32], &fingerprint, PROTOCOL, "Guess"))
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
    assert_eq!(
        reply["agent_proof"],
        proof(&qr_secret(&offer), Role::Agent, &fingerprint)
    );
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
            .send(&pair_message(&[0; 32], &fingerprint, PROTOCOL, "Guess"))
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
            .send(&pair_message(&[0; 32], &seen, PROTOCOL, "Intruder"))
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
async fn a_watching_phone_gets_a_final_revocation_on_last_unpair() {
    let dir = Dir::new();
    let port = free_port();
    let phones = service(&dir, port).await;
    let offer = phones.open_pairing(TTL).await.unwrap();
    let addr = server_addr(&offer);
    let fingerprint = fp_bytes(&offer.fingerprint);
    let token = pair(addr, &offer).await;

    let mut phone = Phone::connect(addr, Some(fingerprint)).await.unwrap();
    phone
        .send(&json!({"v": PROTOCOL, "token": token, "watch_revocation": true}))
        .await;
    assert_eq!(phone.recv().await.unwrap()["ok"], true);
    assert!(phones.unpair(&phones.phones()[0].id).await.unwrap());

    assert_eq!(phone.recv().await, Some(json!({"revoked": true})));
    assert_eq!(phone.recv().await, None);
    assert!(closes(port).await);
}

#[tokio::test]
async fn the_next_ping_reads_the_final_revocation_instead_of_an_answer() {
    let dir = Dir::new();
    let port = free_port();
    let phones = service(&dir, port).await;
    let offer = phones.open_pairing(TTL).await.unwrap();
    let addr = server_addr(&offer);
    let token = pair(addr, &offer).await;
    let mut phone = Phone::connect(addr, Some(fp_bytes(&offer.fingerprint)))
        .await
        .unwrap();
    phone
        .send(&json!({"v": PROTOCOL, "token": token, "watch_revocation": true}))
        .await;
    assert_eq!(phone.recv().await.unwrap()["ok"], true);
    phone.send(&json!({"cmd": "ping"})).await;
    assert_eq!(phone.recv().await.unwrap()["ok"], true);

    assert!(phones.unpair(&phones.phones()[0].id).await.unwrap());
    // A delayed foreground phone may go longer than its usual five-second poll interval.
    tokio::time::sleep(Duration::from_secs(8)).await;
    phone
        .writer
        .write_all(b"{\"cmd\":\"ping\"}\n")
        .await
        .unwrap();
    phone.writer.flush().await.unwrap();
    assert_eq!(phone.recv().await, Some(json!({"revoked": true})));
    assert_eq!(phone.recv().await, None);
    assert!(closes(port).await);
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
    let mut offline = Phone::connect(addr, Some(fingerprint)).await.unwrap();
    offline
        .send(&json!({"v": PROTOCOL, "token": token, "watch_revocation": true}))
        .await;
    assert!(offline.recv().await.is_none());
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

/// A phone as the file keeps it, for a file written before the reset marker existed.
const OLD_PHONE: &str = r#"{"id":"a","name":"x","token_hash":"b","paired_at":1}"#;

#[test]
fn the_reset_marker_is_made_with_the_first_phone_changes_on_demand_and_goes_with_the_last() {
    use construct_agent::phone::paired::Paired;
    let dir = Dir::new();
    std::fs::create_dir_all(&dir.0).unwrap();
    let file = dir.0.join("phones.json");

    let paired = Paired::load(&dir.0).unwrap();
    assert_eq!(paired.reset_marker(), None);
    // Nobody to tell: nothing changes and nothing is written.
    paired.renew_reset().unwrap();
    assert_eq!(paired.reset_marker(), None);
    assert!(!file.exists());

    paired.add("one").unwrap();
    paired.add("two").unwrap();
    let first = paired.reset_marker().unwrap();
    assert!(!first.is_empty());
    // A second phone takes the marker that is current.
    assert_eq!(paired.reset_marker().as_deref(), Some(first.as_str()));

    paired.renew_reset().unwrap();
    let second = paired.reset_marker().unwrap();
    assert_ne!(first, second);

    // It survives a restart.
    let paired = Paired::load(&dir.0).unwrap();
    assert_eq!(paired.reset_marker().as_deref(), Some(second.as_str()));

    let ids: Vec<String> = paired.list().into_iter().map(|p| p.id).collect();
    assert!(paired.remove(&ids[0]).unwrap());
    assert_eq!(paired.reset_marker().as_deref(), Some(second.as_str()));
    assert!(paired.remove(&ids[1]).unwrap());
    assert_eq!(paired.reset_marker(), None);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(!text.contains("reset"), "{text}");
    assert_eq!(Paired::load(&dir.0).unwrap().reset_marker(), None);

    // A phone that pairs after that starts with a marker of its own.
    paired.add("three").unwrap();
    assert_ne!(paired.reset_marker().unwrap(), second);
}

#[test]
fn a_marker_that_cannot_be_written_stays_as_it_was() {
    use construct_agent::phone::paired::Paired;
    let dir = Dir::new();
    std::fs::create_dir_all(&dir.0).unwrap();
    let paired = Paired::load(&dir.0).unwrap();
    paired.add("one").unwrap();
    let before = paired.reset_marker().unwrap();

    std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o500)).unwrap();
    // A user that may write there anyway (root) cannot show a failed write.
    let can_write = std::fs::write(dir.0.join("probe"), "").is_ok();
    let result = paired.renew_reset();
    std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o700)).unwrap();
    if !can_write {
        assert!(result.is_err());
        assert_eq!(paired.reset_marker().as_deref(), Some(before.as_str()));
    }
}

#[test]
fn a_file_from_before_the_marker_is_read_and_gets_one_that_stays_the_same() {
    use construct_agent::phone::paired::Paired;
    let dir = Dir::new();
    std::fs::create_dir_all(&dir.0).unwrap();
    let file = dir.0.join("phones.json");
    std::fs::write(&file, format!(r#"{{"version":1,"phones":[{OLD_PHONE}]}}"#)).unwrap();

    let paired = Paired::load(&dir.0).unwrap();
    assert_eq!(paired.len(), 1);
    let marker = paired.reset_marker().unwrap();
    assert!(std::fs::read_to_string(&file).unwrap().contains(&marker));
    assert_eq!(
        Paired::load(&dir.0).unwrap().reset_marker().as_deref(),
        Some(marker.as_str())
    );

    // A marker in a file that has no phone is dropped, not kept for the next one.
    std::fs::write(&file, r#"{"version":1,"phones":[],"reset":"old"}"#).unwrap();
    assert_eq!(Paired::load(&dir.0).unwrap().reset_marker(), None);
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
    assert_eq!(
        ask("phone-pairing-status", String::new()).await.0,
        json!({"active": true})
    );

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
    assert_eq!(
        ask("phone-pairing-status", String::new()).await.0,
        json!({"active": false})
    );

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
async fn the_desktop_can_page_and_unpair_with_a_large_roster() {
    let dir = Dir::new();
    std::fs::create_dir_all(&dir.0).unwrap();
    let store = construct_agent::phone::paired::Paired::load(&dir.0).unwrap();
    for _ in 0..25 {
        store.add(&"😀".repeat(64)).unwrap();
    }
    let phones = service(&dir, free_port()).await;
    tokio::task::spawn_blocking(move || {
        assert!(phones.command("phone-list", "").len() > 4096);
        let mut cursor = Value::Null;
        let mut ids = Vec::new();
        // Twenty-five phones must fit in four pages. A repeated cursor must fail this test.
        for _ in 0..4 {
            let line = phones.command("phone-list", &json!({"after": cursor}).to_string());
            assert!(line.len() < 4096);
            let reply: Value = serde_json::from_str(&line).unwrap();
            ids.extend(
                reply["phones"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|phone| phone["id"].as_str().unwrap().to_owned()),
            );
            let next = &reply["next"];
            assert!(
                next.is_null()
                    || next
                        .as_str()
                        .is_some_and(|next| cursor.as_str().is_none_or(|id| next > id))
            );
            cursor = next.clone();
            if cursor.is_null() {
                break;
            }
        }
        assert!(cursor.is_null());
        assert_eq!(ids.len(), 25);
        assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
        let reply = phones.command(
            "phone-unpair",
            &json!({"id": ids[0], "ack_only": true}).to_string(),
        );
        assert_eq!(
            serde_json::from_str::<Value>(&reply).unwrap(),
            json!({"ok": true})
        );
        assert_eq!(phones.phones().len(), 24);
    })
    .await
    .unwrap();
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

// ---- Sync: the facts, the chats and the dashboard numbers ----

mod sync {
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicBool, AtomicUsize};

    use connectors::{Author, Record, Role as ChatRole, ToolCall, ToolResult, Turn};
    use construct_agent::activity::rules::{SessionMinutes, minute_number};
    use construct_agent::activity::screen_time::Block;
    use construct_agent::dashboard::Source;
    use construct_agent::dashboard_numbers::{Data as Activity, respond};
    use construct_agent::phone::sync::{
        ChatRead, Data, Engine, FactsRead, ITEM_MAX, Reason, Zone, chat_items, chat_read,
        known_kind,
    };

    use super::*;

    const FACT_TEXT: &str = "Likes tea at four";
    const CHAT_TEXT: &str = "Remember the blue folder";

    struct Inner {
        version: Option<u64>,
        facts: Vec<Value>,
        chats: Vec<ChatRead>,
        sessions: Vec<SessionMinutes>,
        blocks: Vec<Block>,
        document: Value,
    }

    /// Made-up data that a test changes while the endpoint runs. The agent's own settings, with a
    /// key and a model address, sit next to it and must reach no answer.
    struct Fake {
        inner: Mutex<Inner>,
        broken: AtomicBool,
        reads: AtomicUsize,
    }

    fn fact(id: &str, text: &str) -> Value {
        json!({
            "fact_id": id, "entity_kind": "preference", "property": "drink", "value": text,
            "stored_at": "2026-11-01T10:00:00Z",
        })
    }

    fn message(role: &str, text: &str) -> Value {
        json!({"kind": "message", "role": role, "time": "2026-11-01T10:00:00Z", "text": text})
    }

    fn session(name: &str, times: &[&str]) -> SessionMinutes {
        SessionMinutes {
            tool: "claude".into(),
            session: name.into(),
            project: format!("/w/{name}"),
            ticket: String::new(),
            unattended: false,
            area: String::new(),
            collection: String::new(),
            minutes: times.iter().map(|t| minute_number(t).unwrap()).collect(),
        }
    }

    fn small_document() -> Value {
        let mut document = construct_agent::dashboard::default_activity();
        document["payload"]["widgets"]
            .as_array_mut()
            .unwrap()
            .retain(|w| matches!(w["id"].as_str(), Some("total" | "by-project")));
        document
    }

    impl Fake {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                inner: Mutex::new(Inner {
                    version: Some(1),
                    facts: vec![fact("f1", FACT_TEXT)],
                    chats: vec![ChatRead {
                        id: "c1".into(),
                        title: "First".into(),
                        started: "2026-11-01T10:00:00Z".into(),
                        items: vec![message("user", CHAT_TEXT)],
                    }],
                    sessions: vec![session(
                        "a",
                        &[
                            "2026-11-02T09:00:00Z",
                            "2026-11-02T09:01:00Z",
                            "2026-11-04T09:00:00Z",
                        ],
                    )],
                    blocks: Vec::new(),
                    document: small_document(),
                }),
                broken: AtomicBool::new(false),
                reads: AtomicUsize::new(0),
            })
        }

        fn change(&self, f: impl FnOnce(&mut Inner)) {
            f(&mut self.inner.lock().unwrap());
        }

        fn check(&self) -> Result<(), Reason> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            if self.broken.load(Ordering::SeqCst) {
                Err("unavailable")
            } else {
                Ok(())
            }
        }
    }

    impl Data for Fake {
        fn facts(&self) -> Result<FactsRead, Reason> {
            self.check()?;
            let inner = self.inner.lock().unwrap();
            Ok(FactsRead {
                version: inner.version,
                facts: inner.facts.clone(),
            })
        }

        fn chats(&self) -> Result<Vec<ChatRead>, Reason> {
            self.check()?;
            Ok(self.inner.lock().unwrap().chats.clone())
        }

        fn document(&self) -> Result<Option<Value>, Reason> {
            self.check()?;
            Ok(Some(self.inner.lock().unwrap().document.clone()))
        }

        fn activity(&self, _: Source) -> Result<Activity, Reason> {
            self.check()?;
            let inner = self.inner.lock().unwrap();
            Ok(Activity {
                sessions: inner.sessions.clone(),
                blocks: inner.blocks.clone().into(),
            })
        }
    }

    struct Rig {
        endpoint: Endpoint,
        engine: Arc<Engine>,
        fake: Arc<Fake>,
        addr: SocketAddr,
        fingerprint: [u8; 32],
        token: String,
        _dir: Dir,
    }

    async fn rig() -> Rig {
        let dir = Dir::new();
        let endpoint = start(&dir).await;
        let fake = Fake::new();
        let engine = Arc::new(Engine::new(
            Arc::clone(&fake) as Arc<dyn Data>,
            Arc::new(Zone(chrono_tz::Europe::Berlin, "Europe/Berlin".into())),
        ));
        endpoint.set_sync(Some(Arc::clone(&engine)));
        let offer = endpoint.open_pairing(LOCAL, TTL).unwrap();
        let addr = server_addr(&offer);
        let token = pair(addr, &offer).await;
        Rig {
            endpoint,
            engine,
            fake,
            addr,
            fingerprint: fp_bytes(&offer.fingerprint),
            token,
            _dir: dir,
        }
    }

    impl Rig {
        async fn phone(&self) -> Phone {
            let mut phone = Phone::connect(self.addr, Some(self.fingerprint))
                .await
                .unwrap();
            phone
                .send(&json!({"v": PROTOCOL, "token": self.token}))
                .await;
            assert_eq!(phone.recv().await.unwrap()["ok"], true);
            phone
        }

        async fn tokens(&self) -> Value {
            let mut phone = self.phone().await;
            phone.send(&json!({"cmd": "sync-check"})).await;
            phone.recv().await.unwrap()["tokens"].clone()
        }
    }

    /// Every item of a command, over all its pages, and the first page's answer.
    async fn pull(phone: &mut Phone, request: Value) -> (Vec<Value>, Value) {
        let mut items = Vec::new();
        let mut first = None;
        let mut after = Value::Null;
        loop {
            let mut ask = request.clone();
            ask["after"] = after.clone();
            phone.send(&ask).await;
            let line = phone.recv().await.unwrap();
            assert_eq!(line["ok"], true, "{line}");
            assert!(line.to_string().len() < phone_pairing::MAX_LINE);
            items.extend(line["items"].as_array().unwrap().clone());
            after = line["next"].clone();
            first.get_or_insert(line);
            if after.is_null() {
                return (items, first.unwrap());
            }
        }
    }

    #[tokio::test]
    async fn a_paired_phone_gets_the_facts_the_chats_and_the_numbers_and_a_stranger_gets_nothing() {
        let rig = rig().await;
        let mut phone = rig.phone().await;

        let (facts, page) = pull(&mut phone, json!({"cmd": "sync-facts"})).await;
        assert_eq!(page["format"], 1);
        assert_eq!(
            facts,
            vec![json!({
                "kind": "fact", "id": "f1", "entity_kind": "preference", "property": "drink",
                "value": FACT_TEXT, "time": "2026-11-01T10:00:00Z",
            })]
        );

        let (chats, _) = pull(&mut phone, json!({"cmd": "sync-chats"})).await;
        assert_eq!(
            chats,
            vec![json!({"kind": "chat", "id": "c1", "title": "First",
                        "started": "2026-11-01T10:00:00Z", "count": 1})]
        );
        let (items, _) = pull(&mut phone, json!({"cmd": "sync-chat", "id": "c1"})).await;
        assert_eq!(items, vec![message("user", CHAT_TEXT)]);

        phone.send(&json!({"cmd": "sync-document"})).await;
        let document = phone.recv().await.unwrap();
        assert_eq!(document["zone"], "Europe/Berlin");
        assert_eq!(document["document"], small_document());
        let (numbers, _) = pull(&mut phone, json!({"cmd": "sync-dashboard"})).await;
        // Two days and one week hold activity; two widgets each.
        assert_eq!(numbers.len(), 6);
        assert!(numbers.iter().all(|n| n["kind"] == "numbers"));

        // A phone that did not pair gets nothing, whatever it asks.
        for hello in [
            json!({"v": PROTOCOL, "token": "0".repeat(64)}),
            json!({"v": PROTOCOL}),
        ] {
            let mut stranger = Phone::connect(rig.addr, Some(rig.fingerprint))
                .await
                .unwrap();
            stranger.send(&hello).await;
            assert_eq!(stranger.recv().await, None);
            stranger.send(&json!({"cmd": "sync-facts"})).await;
            assert_eq!(stranger.recv().await, None);
        }
        // An agent with no source of data says so, and nothing else.
        rig.endpoint.set_sync(None);
        let mut phone = rig.phone().await;
        phone.send(&json!({"cmd": "sync-facts"})).await;
        assert_eq!(
            phone.recv().await.unwrap(),
            json!({"error": "sync unavailable"})
        );
    }

    #[tokio::test]
    async fn the_token_of_a_part_changes_only_when_that_part_changes() {
        let rig = rig().await;
        let before = rig.tokens().await;
        for part in ["facts", "chats", "dashboard"] {
            assert!(before[part].is_string());
        }
        assert_eq!(
            rig.tokens().await,
            before,
            "nothing changed, nothing differs"
        );

        let changed = |after: &Value, part: &str| {
            ["facts", "chats", "dashboard"]
                .iter()
                .filter(|p| after[**p] != before[**p])
                .copied()
                .collect::<Vec<_>>()
                == [part]
        };

        rig.fake.change(|d| d.version = Some(2));
        assert!(changed(&rig.tokens().await, "facts"));
        rig.fake.change(|d| d.version = Some(1));
        assert_eq!(rig.tokens().await, before);

        rig.fake
            .change(|d| d.chats[0].items.push(message("assistant", "Done")));
        assert!(changed(&rig.tokens().await, "chats"));
        rig.fake.change(|d| {
            d.chats[0].items.pop();
        });
        assert_eq!(rig.tokens().await, before);

        rig.fake.change(|d| {
            d.sessions[0]
                .minutes
                .push(minute_number("2026-11-04T09:01:00Z").unwrap());
        });
        assert!(changed(&rig.tokens().await, "dashboard"));
    }

    #[tokio::test]
    async fn a_part_that_cannot_be_read_has_no_token_and_the_error_holds_no_text() {
        let rig = rig().await;
        rig.fake.broken.store(true, Ordering::SeqCst);
        let mut phone = rig.phone().await;
        phone.send(&json!({"cmd": "sync-check"})).await;
        let check = phone.recv().await.unwrap();
        assert_eq!(
            check["tokens"],
            json!({"facts": null, "chats": null, "dashboard": null})
        );
        let mut seen = vec![check.to_string()];
        for ask in [
            json!({"cmd": "sync-facts"}),
            json!({"cmd": "sync-chats"}),
            json!({"cmd": "sync-chat", "id": FACT_TEXT}),
            json!({"cmd": "sync-dashboard"}),
            json!({"cmd": "sync-facts", "after": "x", "extra": CHAT_TEXT}),
        ] {
            phone.send(&ask).await;
            seen.push(phone.recv().await.unwrap().to_string());
        }
        rig.fake.broken.store(false, Ordering::SeqCst);
        phone
            .send(&json!({"cmd": "sync-chat", "id": FACT_TEXT}))
            .await;
        seen.push(phone.recv().await.unwrap().to_string());
        for line in &seen {
            for secret in [FACT_TEXT, CHAT_TEXT] {
                assert!(!line.contains(secret), "{line}");
            }
        }
        assert_eq!(
            seen.last().unwrap(),
            &json!({"error": "no such chat"}).to_string()
        );
    }

    #[tokio::test]
    async fn a_phone_cannot_ask_for_a_setting_a_model_address_or_a_key() {
        // No command of the channel names a setting: each is answered as an unknown command.
        let rig = rig().await;
        let mut phone = rig.phone().await;
        for command in ["settings", "model", "config", "sync-settings", "sync-key"] {
            phone.send(&json!({"cmd": command})).await;
            assert_eq!(
                phone.recv().await.unwrap(),
                json!({"error": "unknown command"})
            );
        }
        for extra in ["settings", "model", "key"] {
            phone.send(&json!({"cmd": "sync-facts", extra: true})).await;
            assert_eq!(
                phone.recv().await.unwrap(),
                json!({"error": "invalid sync request"})
            );
        }
    }

    fn turn(role: ChatRole, text: &str) -> Record {
        Record::Turn(Turn {
            tool: "chat".into(),
            session: "c1".into(),
            time: "2026-11-01T10:00:00Z".into(),
            folder: String::new(),
            branch: String::new(),
            role,
            author: Author::default(),
            text: text.into(),
            tools: Vec::new(),
        })
    }

    fn call(id: &str, name: &str) -> Vec<Record> {
        vec![
            Record::ToolCall(ToolCall {
                tool: "chat".into(),
                session: "c1".into(),
                time: "2026-11-01T10:00:01Z".into(),
                call_id: id.into(),
                name: name.into(),
                arguments: "{}".into(),
            }),
            Record::ToolResult(ToolResult {
                tool: "chat".into(),
                session: "c1".into(),
                time: "2026-11-01T10:00:02Z".into(),
                call_id: id.into(),
                text: "RESULT TEXT".into(),
            }),
        ]
    }

    #[test]
    fn the_hidden_facts_call_and_every_result_are_not_in_a_chat() {
        let mut records = vec![turn(ChatRole::User, "Hi")];
        records.extend(call("1", chat_core::recall::NAME));
        records.extend(call("2", "read_file"));
        records.push(turn(ChatRole::Assistant, "Hello"));
        let items = chat_items(&records, &|name, _| format!("Ran {name}"));
        assert_eq!(
            items,
            vec![
                json!({"kind": "message", "role": "user", "time": "2026-11-01T10:00:00Z", "text": "Hi"}),
                json!({"kind": "tool", "time": "2026-11-01T10:00:01Z", "line": "Ran read_file"}),
                json!({"kind": "message", "role": "assistant", "time": "2026-11-01T10:00:00Z", "text": "Hello"}),
            ]
        );
        assert!(!json!(items).to_string().contains("RESULT TEXT"));
        assert!(!json!(items).to_string().contains(chat_core::recall::NAME));
    }

    #[tokio::test]
    async fn an_item_of_a_kind_nobody_knows_travels_and_an_old_reader_skips_it() {
        let rig = rig().await;
        rig.fake.change(|d| {
            d.chats[0].items.insert(
                1,
                json!({"kind": "sticker", "time": "2026-11-01T10:00:00Z", "shape": "star"}),
            );
            d.chats[0].items.push(message("assistant", "after"));
        });
        let mut phone = rig.phone().await;
        let (items, page) = pull(&mut phone, json!({"cmd": "sync-chat", "id": "c1"})).await;
        assert_eq!(page["format"], 1);
        assert_eq!(items.len(), 3);
        assert_eq!(items[1]["kind"], "sticker");
        // What a reader of format 1 does: keep the kinds it knows.
        let kept: Vec<&Value> = items.iter().filter(|i| known_kind(i).is_some()).collect();
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[1]["text"], "after");
    }

    #[tokio::test]
    async fn a_long_list_comes_in_pages_that_fit_a_line_and_a_long_text_is_cut_and_says_so() {
        let rig = rig().await;
        rig.fake.change(|d| {
            d.facts = (0..400)
                .map(|i| fact(&format!("f{i}"), &"x".repeat(100)))
                .collect();
            d.chats[0].items = vec![message("user", &"y".repeat(9000))];
        });
        let mut phone = rig.phone().await;
        phone.send(&json!({"cmd": "sync-facts"})).await;
        let first = phone.recv().await.unwrap();
        assert!(first["next"].is_u64(), "400 facts are more than one page");
        let (facts, _) = pull(&mut phone, json!({"cmd": "sync-facts"})).await;
        assert_eq!(facts.len(), 400);
        assert_eq!(facts[399]["id"], "f399");
        let (items, _) = pull(&mut phone, json!({"cmd": "sync-chat", "id": "c1"})).await;
        assert_eq!(items[0]["cut"], true);
        assert_eq!(items[0]["text"].as_str().unwrap().len(), 4000);
    }

    /// What `dashboard-numbers` answers for an input, from the same sessions.
    fn expected(rig: &Rig, input: &Value) -> Value {
        let inner = rig.fake.inner.lock().unwrap();
        let line = respond(&input.to_string(), Some(&inner.document), |_| {
            Ok(inner.sessions.clone())
        });
        serde_json::from_str(&line).unwrap()
    }

    #[tokio::test]
    async fn the_numbers_equal_dashboard_numbers_for_a_day_and_a_week_across_a_clock_change() {
        let rig = rig().await;
        // Berlin ends summer time on 2026-10-25 (a day of 25 hours) and starts it on 2026-03-29
        // (a day of 23 hours); each holds activity late in the evening and early next morning.
        rig.fake.change(|d| {
            d.sessions = vec![
                session(
                    "a",
                    &[
                        "2026-10-24T21:50:00Z",
                        "2026-10-24T21:51:00Z",
                        "2026-10-25T10:00:00Z",
                        "2026-10-25T10:01:00Z",
                        "2026-10-25T22:59:00Z",
                        "2026-10-25T23:00:00Z",
                        "2026-03-28T23:30:00Z",
                        "2026-03-28T23:31:00Z",
                        "2026-03-29T21:59:00Z",
                        "2026-03-29T22:00:00Z",
                    ],
                ),
                session("b", &["2026-10-25T10:00:00Z", "2026-10-25T10:02:00Z"]),
            ];
        });
        let mut phone = rig.phone().await;
        let (numbers, _) = pull(&mut phone, json!({"cmd": "sync-dashboard"})).await;
        assert!(!numbers.is_empty());
        let mut days = 0;
        let mut weeks = 0;
        for item in &numbers {
            assert_eq!(item["answer"], expected(&rig, &item["input"]), "{item}");
            let seven = item["input"]["days"].as_array().unwrap().len() == 7;
            assert_eq!(item["period"] == "week", seven);
            if seven {
                weeks += 1;
                // Monday to Sunday.
                assert!(item["input"]["days"][0]["label"].as_str().is_some());
            } else {
                days += 1;
            }
        }
        assert!(days > 0 && weeks > 0);

        let long_day = numbers
            .iter()
            .find(|n| n["period"] == "day" && n["input"]["days"][0]["label"] == "2026-10-25")
            .unwrap();
        // 25 hours, in UTC, with the label of the computer's date.
        assert_eq!(
            long_day["input"]["days"][0],
            json!({"label": "2026-10-25", "start": "2026-10-24T22:00:00Z", "end": "2026-10-25T23:00:00Z"})
        );
        let short_day = numbers
            .iter()
            .find(|n| n["period"] == "day" && n["input"]["days"][0]["label"] == "2026-03-29")
            .unwrap();
        assert_eq!(
            short_day["input"]["days"][0],
            json!({"label": "2026-03-29", "start": "2026-03-28T23:00:00Z", "end": "2026-03-29T22:00:00Z"})
        );
        let week = numbers
            .iter()
            .find(|n| n["period"] == "week" && n["input"]["days"][0]["label"] == "2026-10-19")
            .unwrap();
        assert_eq!(week["input"]["days"][6]["label"], "2026-10-25");
        assert_eq!(week["input"]["days"][6]["end"], "2026-10-25T23:00:00Z");
    }

    /// 300 sessions of one project on one day, each two minutes, three minutes apart; and one
    /// session of a day far older than the others.
    fn heavy_day() -> Vec<SessionMinutes> {
        let first = minute_number("2026-11-02T08:00:00Z").unwrap();
        let mut sessions: Vec<SessionMinutes> = (0..300)
            .map(|i| {
                let mut s = session(&format!("s{i:03}-private-words-of-a-message"), &[]);
                s.project = "/w/shop".into();
                s.minutes = vec![first + 3 * i, first + 3 * i + 1];
                s
            })
            .collect();
        sessions.push(session(
            "old",
            &["2026-01-10T09:00:00Z", "2026-01-10T09:01:00Z"],
        ));
        sessions
    }

    #[tokio::test]
    async fn the_sessions_of_each_group_come_in_pages_with_no_text_and_add_up_to_the_group() {
        let rig = rig().await;
        rig.fake.change(|d| d.sessions = heavy_day());
        let mut phone = rig.phone().await;
        let (numbers, _) = pull(&mut phone, json!({"cmd": "sync-dashboard"})).await;
        let (items, first) = pull(&mut phone, json!({"cmd": "sync-dashboard-sessions"})).await;

        assert!(items.iter().all(|i| i["kind"] == "sessions"), "{items:?}");
        // A heavy day does not fit one item, and no page is longer than a line of the channel
        // (`pull` checks it).
        assert!(items.len() > 3, "{}", items.len());
        let bytes: usize = items.iter().map(|i| i.to_string().len()).sum();
        eprintln!(
            "heavy day and its week: 300 sessions, {} items, {bytes} bytes",
            items.len()
        );
        assert!(!first["token"].is_null());
        // No text of a message: a row has these fields and nothing else.
        let fields: BTreeSet<&str> = [
            "start", "end", "minutes", "tool", "project", "ticket", "session",
        ]
        .into_iter()
        .collect();
        let mut seen = 0;
        for item in &items {
            for row in item["rows"].as_array().unwrap() {
                let keys: BTreeSet<&str> = row
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect();
                assert_eq!(keys, fields);
                // Only the first characters of the id are sent, never the whole of it.
                assert!(row["session"].as_str().unwrap().len() <= 8);
                seen += 1;
            }
        }
        // The 300 sessions are behind the day and behind the week that holds it.
        assert_eq!(seen, 600);
        assert!(
            !items
                .iter()
                .any(|i| i.to_string().contains("private-words"))
        );

        // For the days of a numbers entry that have sessions, those of a group add up to it.
        let mut checked = 0;
        for entry in numbers.iter().filter(|n| n["widget"] == "by-project") {
            for group in entry["answer"]["groups"].as_array().unwrap() {
                let rows: Vec<&Value> = items
                    .iter()
                    .filter(|i| i["input"] == entry["input"] && i["group"] == group["key"])
                    .flat_map(|i| i["rows"].as_array().unwrap())
                    .collect();
                if rows.is_empty() {
                    continue;
                }
                let sum: u64 = rows.iter().map(|r| r["minutes"].as_u64().unwrap()).sum();
                assert_eq!(
                    sum.to_string(),
                    format!("{:.0}", group["minutes"].as_f64().unwrap()),
                    "{entry}"
                );
                checked += 1;
            }
        }
        // The day and the week of the heavy day.
        assert_eq!(checked, 2);
        // The numbers pages stay as they were: they hold no sessions.
        assert!(numbers.iter().all(|n| n["kind"] == "numbers"));
    }

    #[tokio::test]
    async fn the_sessions_are_not_sent_for_days_long_before_the_newest_and_a_changed_part_is_said()
    {
        let rig = rig().await;
        rig.fake.change(|d| d.sessions = heavy_day());
        let mut phone = rig.phone().await;
        let (numbers, first) = pull(&mut phone, json!({"cmd": "sync-dashboard"})).await;
        let (items, sessions_first) =
            pull(&mut phone, json!({"cmd": "sync-dashboard-sessions"})).await;
        let old: Vec<&Value> = numbers
            .iter()
            .filter(|n| {
                n["input"]["days"][0]["label"] == "2026-01-05"
                    || n["input"]["days"][0]["label"] == "2026-01-10"
            })
            .collect();
        assert!(!old.is_empty());
        assert!(
            items
                .iter()
                .all(|i| old.iter().all(|o| o["input"] != i["input"]))
        );

        // The same token as the numbers; an older one is told it changed.
        assert_eq!(sessions_first["token"], first["token"]);
        phone
            .send(&json!({"cmd": "sync-dashboard-sessions", "token": "stale"}))
            .await;
        let reply = phone.recv().await.unwrap();
        assert_eq!(reply["changed"], true);
    }

    #[tokio::test]
    async fn a_day_is_counted_once_and_again_only_when_its_activity_changes() {
        let rig = rig().await;
        let mut phone = rig.phone().await;
        let (first, _) = pull(&mut phone, json!({"cmd": "sync-dashboard"})).await;
        let counted = rig.engine.counted();
        assert_eq!(counted, first.len());

        // The same copy again, and a check: nothing is counted.
        let (again, _) = pull(&mut phone, json!({"cmd": "sync-dashboard"})).await;
        rig.tokens().await;
        assert_eq!(again, first);
        assert_eq!(rig.engine.counted(), counted);

        // One more minute on 2026-11-04: that day and its week are counted again, in both
        // widgets, and the day of 2026-11-02 is not.
        rig.fake.change(|d| {
            d.sessions[0]
                .minutes
                .push(minute_number("2026-11-04T09:02:00Z").unwrap());
        });
        let (after, _) = pull(&mut phone, json!({"cmd": "sync-dashboard"})).await;
        assert_eq!(rig.engine.counted(), counted + 4);
        assert_eq!(after.len(), first.len());
        let changed: Vec<_> = after.iter().zip(&first).filter(|(a, b)| a != b).collect();
        assert_eq!(changed.len(), 4);
    }

    #[tokio::test]
    async fn the_newest_days_come_first() {
        let rig = rig().await;
        let mut phone = rig.phone().await;
        let (numbers, _) = pull(&mut phone, json!({"cmd": "sync-dashboard"})).await;
        let days: Vec<&str> = numbers
            .iter()
            .filter(|n| n["period"] == "day")
            .map(|n| n["input"]["days"][0]["label"].as_str().unwrap())
            .collect();
        assert_eq!(days[0], "2026-11-04");
        assert_eq!(*days.last().unwrap(), "2026-11-02");
    }

    #[tokio::test]
    async fn no_line_is_over_the_limit_whatever_an_item_holds() {
        let rig = rig().await;
        rig.fake.change(|d| {
            d.facts = vec![
                // A value of 20 KiB, as text and as an object.
                fact("big-text", &"v".repeat(20 * 1024)),
                json!({"fact_id": "big-object", "entity_kind": "skill", "property": "skills",
                       "value": {"name": "x".repeat(20 * 1024)}, "stored_at": "2026-11-01T10:00:00Z"}),
                // Characters that serialise to six bytes each.
                fact("controls", &"\u{1}".repeat(4000)),
            ];
            d.chats[0].items = vec![message("user", &"\u{1}".repeat(4000))];
        });
        let mut phone = rig.phone().await;
        let (facts, _) = pull(&mut phone, json!({"cmd": "sync-facts"})).await;
        assert_eq!(facts.len(), 3);
        for fact in &facts {
            assert!(fact.to_string().len() <= ITEM_MAX, "{}", fact["id"]);
            assert_eq!(fact["cut"], true);
        }
        assert!(facts[1]["value"].is_string());
        let (items, _) = pull(&mut phone, json!({"cmd": "sync-chat", "id": "c1"})).await;
        assert!(items[0].to_string().len() <= ITEM_MAX);
        assert_eq!(items[0]["cut"], true);
    }

    #[tokio::test]
    async fn the_document_has_a_line_of_its_own_and_the_pages_of_numbers_stay_small() {
        let rig = rig().await;
        // A document of about 9 KiB: its widgets are many and its project names long.
        rig.fake.change(|d| {
            d.document["payload"]["project_names"] = json!(
                (0..120)
                    .map(|i| (format!("/w/p{i}"), "n".repeat(60)))
                    .map(|(k, v)| (k, json!(v)))
                    .collect::<serde_json::Map<String, Value>>()
            );
        });
        let mut phone = rig.phone().await;
        phone.send(&json!({"cmd": "sync-document"})).await;
        let line = phone.recv().await.unwrap();
        assert!(
            line.to_string().len() > 8 * 1024 && line.to_string().len() < phone_pairing::MAX_LINE
        );
        assert_eq!(line["document"], rig.fake.inner.lock().unwrap().document);
        let (numbers, _) = pull(&mut phone, json!({"cmd": "sync-dashboard"})).await;
        assert!(!numbers.is_empty());
        // A document too big for a line is refused in fixed words.
        rig.fake
            .change(|d| d.document["payload"]["project_names"] = json!({"a": "z".repeat(20_000)}));
        phone.send(&json!({"cmd": "sync-document"})).await;
        assert_eq!(
            phone.recv().await.unwrap(),
            json!({"error": "dashboard document too large"})
        );
    }

    #[tokio::test]
    async fn a_page_asked_with_an_old_token_says_changed_and_gives_nothing() {
        let rig = rig().await;
        rig.fake.change(|d| {
            d.facts = (0..400)
                .map(|i| fact(&format!("f{i}"), &"x".repeat(100)))
                .collect();
        });
        let mut phone = rig.phone().await;
        phone.send(&json!({"cmd": "sync-facts"})).await;
        let first = phone.recv().await.unwrap();
        let (token, next) = (first["token"].clone(), first["next"].clone());
        // Nothing changed: the same token gets the next page.
        phone
            .send(&json!({"cmd": "sync-facts", "after": next, "token": token}))
            .await;
        assert_eq!(phone.recv().await.unwrap()["ok"], true);
        // The profile changed between two pages.
        rig.fake.change(|d| d.version = Some(7));
        phone
            .send(&json!({"cmd": "sync-facts", "after": next, "token": token}))
            .await;
        let answer = phone.recv().await.unwrap();
        assert_eq!(answer["changed"], true);
        assert_eq!(answer["items"], json!([]));
        assert_ne!(answer["token"], token);
        // The same for the numbers, which are not counted for a copy that is out of date.
        phone.send(&json!({"cmd": "sync-dashboard"})).await;
        let first = phone.recv().await.unwrap();
        rig.fake.change(|d| {
            d.sessions[0]
                .minutes
                .push(minute_number("2026-11-04T09:03:00Z").unwrap());
        });
        let counted = rig.engine.counted();
        phone
            .send(&json!({"cmd": "sync-dashboard", "after": 0, "token": first["token"]}))
            .await;
        assert_eq!(phone.recv().await.unwrap()["changed"], true);
        assert_eq!(rig.engine.counted(), counted);
    }

    #[tokio::test]
    async fn the_facts_token_without_a_version_follows_the_facts() {
        let rig = rig().await;
        rig.fake.change(|d| d.version = None);
        let before = rig.tokens().await["facts"].clone();
        assert!(before.is_string());
        assert_eq!(rig.tokens().await["facts"], before);
        rig.fake
            .change(|d| d.facts.push(fact("f2", "Likes coffee")));
        assert_ne!(rig.tokens().await["facts"], before);
    }

    #[tokio::test]
    async fn a_page_that_is_out_of_time_answers_what_it_counted_and_a_next() {
        let rig = rig().await;
        let engine = Arc::new(
            Engine::new(
                Arc::clone(&rig.fake) as Arc<dyn Data>,
                Arc::new(Zone(chrono_tz::Europe::Berlin, "Europe/Berlin".into())),
            )
            .with_page_time(Duration::ZERO),
        );
        rig.endpoint.set_sync(Some(Arc::clone(&engine)));
        let mut phone = rig.phone().await;
        phone.send(&json!({"cmd": "sync-dashboard"})).await;
        let page = phone.recv().await.unwrap();
        assert_eq!(page["items"].as_array().unwrap().len(), 1);
        assert_eq!(page["next"], 1);
        // The pages together are the whole copy, as with the time to count them.
        let (all, _) = pull(&mut phone, json!({"cmd": "sync-dashboard"})).await;
        assert_eq!(all.len(), 6);
    }

    /// What the days of a window change when only the stamps are looked at: the answers counted
    /// after a change, by what changed.
    async fn recounted(rig: &Rig, change: impl FnOnce(&mut Inner)) -> usize {
        let mut phone = rig.phone().await;
        pull(&mut phone, json!({"cmd": "sync-dashboard"})).await;
        let before = rig.engine.counted();
        rig.fake.change(change);
        pull(&mut phone, json!({"cmd": "sync-dashboard"})).await;
        rig.engine.counted() - before
    }

    #[tokio::test]
    async fn the_stamp_of_a_day_follows_what_counts_in_it() {
        // Two widgets, so a day and a week recounted are four answers.
        // A minute next to midnight (Berlin: 23:58 on the 4th) reaches into the 5th.
        let rig = rig().await;
        rig.fake.change(|d| {
            d.sessions[0]
                .minutes
                .push(minute_number("2026-11-05T09:00:00Z").unwrap());
        });
        let neighbour = recounted(&rig, |d| {
            d.sessions[0]
                .minutes
                .push(minute_number("2026-11-04T22:58:00Z").unwrap());
        })
        .await;
        // The 4th (own), its week, and the 5th (it is within reach of the new minute), two widgets.
        assert_eq!(neighbour, 6);
        // A minute at noon of the 4th reaches no other day: only that day and its week.
        let rig = self::rig().await;
        rig.fake.change(|d| {
            d.sessions[0]
                .minutes
                .push(minute_number("2026-11-05T09:00:00Z").unwrap());
        });
        let far = recounted(&rig, |d| {
            d.sessions[0]
                .minutes
                .push(minute_number("2026-11-04T11:00:00Z").unwrap());
        })
        .await;
        assert_eq!(far, 4);

        // A session of a new project renames none of the days it is not in, yet the names are
        // chosen over all the sessions, so every answer is counted again.
        let rig = self::rig().await;
        let all = recounted(&rig, |d| {
            d.sessions.push(session("b", &["2026-11-03T09:00:00Z"]));
        })
        .await;
        assert_eq!(all, 8);

        // The document changed: every answer is counted again.
        let rig = self::rig().await;
        let document = recounted(&rig, |d| {
            d.document["payload"]["hidden_projects"] = json!(["/w/a"]);
        })
        .await;
        assert_eq!(document, 6);
    }

    fn block(start: &str, seconds: u64, app: &str) -> Block {
        Block {
            start: start.into(),
            seconds,
            app: app.into(),
            site: String::new(),
            project: String::new(),
            ticket: String::new(),
        }
    }

    #[tokio::test]
    async fn the_stamp_of_a_day_of_the_time_on_screen_follows_its_blocks() {
        let rig = rig().await;
        rig.fake.change(|d| {
            let mut document = construct_agent::dashboard::default_activity_with_screen();
            document["payload"]["widgets"]
                .as_array_mut()
                .unwrap()
                .retain(|w| w["id"] == "screen-total");
            d.document = document;
            d.sessions.clear();
            d.blocks = vec![block("2026-11-02T09:00:00Z", 600, "Editor")];
        });
        // One day and its week; one widget.
        let changed_seconds = recounted(&rig, |d| d.blocks[0].seconds = 900).await;
        assert_eq!(changed_seconds, 2);
        let new_day = recounted(&rig, |d| {
            d.blocks.push(block("2026-11-03T09:00:00Z", 60, "Editor"));
        })
        .await;
        // The new day, and its week, which holds both days now.
        assert_eq!(new_day, 2);
        let mut phone = rig.phone().await;
        let (numbers, _) = pull(&mut phone, json!({"cmd": "sync-dashboard"})).await;
        assert_eq!(numbers.len(), 3);
        assert!(numbers.iter().all(|n| n["answer"]["source"] == "on_screen"));
    }

    #[tokio::test]
    async fn an_answer_of_numbers_that_is_too_big_for_a_line_loses_groups_and_says_so() {
        let rig = rig().await;
        rig.fake.change(|d| {
            // 600 projects with long names in one day: a table by project of about 70 KiB.
            d.sessions = (0..600)
                .map(|i| {
                    let mut s = session(&format!("s{i}"), &["2026-11-02T09:00:00Z"]);
                    s.project = format!("/work/{}/{i}", "p".repeat(60));
                    s
                })
                .collect();
        });
        let mut phone = rig.phone().await;
        let (numbers, _) = pull(&mut phone, json!({"cmd": "sync-dashboard"})).await;
        let table = numbers
            .iter()
            .find(|n| n["widget"] == "by-project" && n["period"] == "day")
            .unwrap();
        assert!(table.to_string().len() <= ITEM_MAX + 11);
        assert_eq!(table["cut"], true);
        assert!(!table["answer"]["groups"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_zone_is_part_of_the_dashboard_token() {
        let rig = rig().await;
        let berlin = rig.tokens().await["dashboard"].clone();
        let other = Arc::new(Engine::new(
            Arc::clone(&rig.fake) as Arc<dyn Data>,
            Arc::new(Zone(chrono_tz::Asia::Tokyo, "Asia/Tokyo".into())),
        ));
        rig.endpoint.set_sync(Some(other));
        assert_ne!(rig.tokens().await["dashboard"], berlin);
    }

    #[test]
    fn a_chat_is_titled_by_its_first_message_of_the_person_and_an_empty_one_is_left_out() {
        let records = vec![
            turn(ChatRole::Assistant, "Welcome"),
            turn(ChatRole::User, &"t".repeat(100)),
        ];
        let chat = chat_read("c1".into(), &records, &|_, _| String::new()).unwrap();
        assert_eq!(chat.title, "t".repeat(60));
        assert_eq!(chat.started, "2026-11-01T10:00:00Z");
        assert_eq!(chat.items.len(), 2);
        assert!(chat_read("c2".into(), &[], &|_, _| String::new()).is_none());
    }

    /// The `reset` of a check, over a new connection of the phone with this token.
    async fn reset_seen(rig: &Rig, token: &str) -> String {
        let mut phone = Phone::connect(rig.addr, Some(rig.fingerprint))
            .await
            .unwrap();
        phone.send(&json!({"v": PROTOCOL, "token": token})).await;
        assert_eq!(phone.recv().await.unwrap()["ok"], true);
        phone.send(&json!({"cmd": "sync-check"})).await;
        let answer = phone.recv().await.unwrap();
        assert_eq!(answer["ok"], true, "{answer}");
        answer["reset"].as_str().unwrap().to_owned()
    }

    #[tokio::test]
    async fn a_phone_that_was_away_for_the_erase_sees_a_new_marker_and_one_that_pairs_later_the_current()
     {
        let rig = rig().await;
        let before = reset_seen(&rig, &rig.token).await;
        // Nothing erased: the marker does not move between checks.
        assert_eq!(reset_seen(&rig, &rig.token).await, before);

        // The person erases while the phone is away: the data goes, the marker changes.
        rig.fake.change(|d| {
            d.facts.clear();
            d.chats.clear();
        });
        rig.endpoint.reset_phones().unwrap();

        let after = reset_seen(&rig, &rig.token).await;
        assert_ne!(after, before);
        // The checks give no facts and no chats now.
        let mut phone = rig.phone().await;
        let (facts, _) = pull(&mut phone, json!({"cmd": "sync-facts"})).await;
        assert!(facts.is_empty());
        let (chats, _) = pull(&mut phone, json!({"cmd": "sync-chats"})).await;
        assert!(chats.is_empty());

        // A phone that pairs after the erase takes the current marker.
        let offer = rig.endpoint.open_pairing(LOCAL, TTL).unwrap();
        let later = pair(server_addr(&offer), &offer).await;
        assert_eq!(reset_seen(&rig, &later).await, after);
    }

    #[tokio::test]
    async fn every_command_is_refused_while_an_erasure_runs_and_answers_again_after() {
        let rig = rig().await;
        let erasing = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&erasing);
        rig.endpoint.set_sync(Some(Arc::new(
            Engine::new(
                Arc::clone(&rig.fake) as Arc<dyn Data>,
                Arc::new(Zone(chrono_tz::Europe::Berlin, "Europe/Berlin".into())),
            )
            .with_erasing(Arc::new(move || flag.load(Ordering::SeqCst))),
        )));
        let mut phone = rig.phone().await;
        for command in ["sync-check", "sync-facts", "sync-chats", "sync-dashboard"] {
            phone.send(&json!({"cmd": command})).await;
            let answer = phone.recv().await.unwrap();
            assert_eq!(answer, json!({"error": "erasing"}), "{command}");
        }
        erasing.store(false, Ordering::SeqCst);
        phone.send(&json!({"cmd": "sync-check"})).await;
        let answer = phone.recv().await.unwrap();
        assert_eq!(answer["ok"], true, "{answer}");
        assert!(answer["reset"].is_string());
    }

    /// The first copy of a history of about 140,000 prompts in 30 days, timed. Run it with
    /// `cargo test -p construct-agent --test phone --release -- --ignored --nocapture heavy`.
    #[tokio::test]
    #[ignore = "a measurement, not a check"]
    async fn heavy_history_first_copy() {
        let rig = rig().await;
        let start = minute_number("2026-10-20T00:00:00Z").unwrap();
        // 140,000 prompts, a prompt a minute in bursts, over 30 days and 20 sessions.
        let mut sessions: Vec<SessionMinutes> =
            (0..20).map(|i| session(&format!("s{i}"), &[])).collect();
        let mut minute = 0_i64;
        let mut n = 0_usize;
        while n < 140_000 {
            let day = minute / 1440;
            let at = start + day * 1440 + 480 + (minute % 1440) % 600;
            sessions[n % 20].minutes.push(at);
            n += 1;
            minute += 1;
            if minute >= 30 * 1440 {
                minute = 0;
            }
        }
        rig.fake.change(|d| d.sessions = sessions);
        let mut phone = rig.phone().await;
        let began = std::time::Instant::now();
        let (numbers, _) = pull(&mut phone, json!({"cmd": "sync-dashboard"})).await;
        println!(
            "first copy: {} answers in {:?}; the second: {:?}",
            numbers.len(),
            began.elapsed(),
            {
                let again = std::time::Instant::now();
                pull(&mut phone, json!({"cmd": "sync-dashboard"})).await;
                again.elapsed()
            }
        );
    }
}
