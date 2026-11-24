//! TLS and pairing authentication for the phone channel. Kotlin owns the pairing state.
use std::ffi::c_char;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::ptr;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use phone_pairing::{
    BINDING_LEN, CODE_REJECTED, CodeChallenge, CodeRequest, EXPORTER_LABEL, Role, TYPED_CAPABILITY,
    qr_proof,
};
use ring::digest;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, Error, SignatureScheme, StreamOwned,
};
use serde_json::{Value, json};

use super::{argument, c_string, guarded};

const MAX_LINE: usize = 16 * 1024;
const TIMEOUT: Duration = Duration::from_secs(5);

struct Peer {
    provider: Arc<CryptoProvider>,
    pin: Option<[u8; 32]>,
    fingerprint: OnceLock<[u8; 32]>,
}

impl std::fmt::Debug for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PhonePeer")
    }
}

impl ServerCertVerifier for Peer {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let value = digest::digest(&digest::SHA256, end_entity.as_ref());
        let fingerprint = <[u8; 32]>::try_from(value.as_ref())
            .map_err(|_| Error::General("invalid fingerprint".into()))?;
        if self.pin.is_some_and(|pin| pin != fingerprint) {
            return Err(Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ));
        }
        let _ = self.fingerprint.set(fingerprint);
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::General("TLS 1.3 required".into()))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(
            message,
            certificate,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// One deadline for the entire handshake or exchange, including a fragmented line.
struct DeadlineSocket {
    stream: TcpStream,
    deadline: Instant,
}

impl DeadlineSocket {
    fn remaining(&self) -> io::Result<Duration> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        Ok(left)
    }
}

impl Read for DeadlineSocket {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.stream.set_read_timeout(Some(self.remaining()?))?;
        self.stream.read(bytes)
    }
}

impl Write for DeadlineSocket {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        self.stream.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

struct Channel {
    wire: BufReader<StreamOwned<ClientConnection, DeadlineSocket>>,
    fingerprint: [u8; 32],
}

enum OpenError {
    Unavailable,
    WrongComputer,
    ComputerUpdate,
}

enum PairError {
    Unavailable,
    Refused,
}

impl Channel {
    fn open(host: &str, port: u16, pin: Option<[u8; 32]>) -> Result<Self, OpenError> {
        let deadline = Instant::now() + TIMEOUT;
        let host: IpAddr = host.parse().map_err(|_| OpenError::Unavailable)?;
        let stream = TcpStream::connect_timeout(&SocketAddr::new(host, port), TIMEOUT)
            .map_err(|_| OpenError::Unavailable)?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let peer = Arc::new(Peer {
            provider: Arc::clone(&provider),
            pin,
            fingerprint: OnceLock::new(),
        });
        let mut config = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|_| OpenError::Unavailable)?
            .dangerous()
            .with_custom_certificate_verifier(peer.clone())
            .with_no_client_auth();
        config.enable_sni = false;
        config.resumption = rustls::client::Resumption::disabled();
        if pin.is_none() {
            config.alpn_protocols = vec![TYPED_CAPABILITY.to_vec()];
        }
        let mut tls = ClientConnection::new(Arc::new(config), ServerName::IpAddress(host.into()))
            .map_err(|_| OpenError::Unavailable)?;
        let mut socket = DeadlineSocket { stream, deadline };
        while tls.is_handshaking() {
            if let Err(error) = tls.complete_io(&mut socket) {
                let error = error
                    .get_ref()
                    .and_then(|error| error.downcast_ref::<Error>());
                return Err(match error {
                    Some(Error::InvalidCertificate(
                        rustls::CertificateError::ApplicationVerificationFailure,
                    )) => OpenError::WrongComputer,
                    Some(
                        Error::AlertReceived(rustls::AlertDescription::NoApplicationProtocol)
                        | Error::NoApplicationProtocol,
                    ) if pin.is_none() => OpenError::ComputerUpdate,
                    _ => OpenError::Unavailable,
                });
            }
        }
        if pin.is_none() && tls.alpn_protocol() != Some(TYPED_CAPABILITY) {
            return Err(OpenError::ComputerUpdate);
        }
        Ok(Self {
            wire: BufReader::new(StreamOwned::new(tls, socket)),
            fingerprint: *peer.fingerprint.get().ok_or(OpenError::Unavailable)?,
        })
    }

    fn pair_code(&mut self, code: &str, version: u64, name: &str) -> Result<String, PairError> {
        if code.len() != 8 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(PairError::Unavailable);
        }
        if self.wire.get_ref().conn.alpn_protocol() != Some(TYPED_CAPABILITY) {
            return Err(PairError::Unavailable);
        }
        let binding = self
            .wire
            .get_ref()
            .conn
            .export_keying_material([0; BINDING_LEN], EXPORTER_LABEL, None)
            .map_err(|_| PairError::Unavailable)?;
        let (request, message) = CodeRequest::new(code.as_bytes()).ok_or(PairError::Unavailable)?;
        let hello = json!({"v": version, "pair_code": {"request": hex::encode(message)}});
        let response = self.code_reply(&hello)?;
        let challenge = response
            .get("challenge")
            .and_then(Value::as_str)
            .ok_or(PairError::Unavailable)?;
        let challenge = hex::decode(challenge).map_err(|_| PairError::Unavailable)?;
        let challenge = CodeChallenge::parse(&challenge).ok_or(PairError::Unavailable)?;
        let proof = request
            .confirm(code.as_bytes(), challenge, &binding, version)
            .ok_or(PairError::Unavailable)?;
        let confirmation = json!({"confirmation": hex::encode(proof), "name": name});
        Ok(self.code_reply(&confirmation)?.to_string())
    }

    fn code_reply(&mut self, request: &Value) -> Result<Value, PairError> {
        let line = self
            .exchange(&request.to_string())
            .map_err(|_| PairError::Unavailable)?
            .ok_or(PairError::Unavailable)?;
        let reply: Value = serde_json::from_str(&line).map_err(|_| PairError::Unavailable)?;
        if reply == json!({"error": CODE_REJECTED}) {
            return Err(PairError::Refused);
        }
        Ok(reply)
    }

    fn exchange(&mut self, line: &str) -> io::Result<Option<String>> {
        if line.len() > MAX_LINE || line.contains('\n') {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let stream = self.wire.get_mut();
        stream.sock.deadline = Instant::now() + TIMEOUT;
        // The agent may have already sent the final revocation and closed the socket.
        let _ = stream
            .write_all(line.as_bytes())
            .and_then(|()| stream.write_all(b"\n"))
            .and_then(|()| stream.flush());
        let mut reply = Vec::new();
        (&mut self.wire)
            .take(MAX_LINE as u64 + 1)
            .read_until(b'\n', &mut reply)?;
        if reply.is_empty() {
            // rustls returns UnexpectedEof for a TCP close without a TLS close_notify.
            return Ok(None);
        }
        if reply.pop() != Some(b'\n') {
            return Err(io::ErrorKind::InvalidData.into());
        }
        String::from_utf8(reply)
            .map(Some)
            .map_err(|_| io::ErrorKind::InvalidData.into())
    }
}

fn hex32(value: &str) -> Option<[u8; 32]> {
    let mut bytes = [0; 32];
    hex::decode_to_slice(value, &mut bytes).ok()?;
    Some(bytes)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn cf_phone_is_numeric_address(host: *const c_char) -> u8 {
    // SAFETY: the caller's host is a C string valid for this call.
    u8::from(unsafe { argument(host) }.is_some_and(|host| host.parse::<IpAddr>().is_ok()))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn cf_phone_open(
    host: *const c_char,
    port: u16,
    pin: *const c_char,
    out: *mut *mut Channel,
) -> i32 {
    guarded(
        || {
            // SAFETY: strings at the C boundary are valid for the duration of the call.
            let (Some(host), Some(out)) = (unsafe { (argument(host), out.as_mut()) }) else {
                return 1;
            };
            let pin = if pin.is_null() {
                None
            } else {
                // SAFETY: the non-null pin is a valid C string.
                let Some(bytes) = (unsafe { argument(pin) }).as_deref().and_then(hex32) else {
                    return 1;
                };
                Some(bytes)
            };
            match Channel::open(&host, port, pin) {
                Ok(channel) => {
                    *out = Box::into_raw(Box::new(channel));
                    0
                }
                Err(OpenError::Unavailable) => 1,
                Err(OpenError::WrongComputer) => 2,
                Err(OpenError::ComputerUpdate) => 4,
            }
        },
        || 1,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn cf_phone_pair_code(
    channel: *mut Channel,
    code: *const c_char,
    version: u64,
    name: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    guarded(
        || {
            // SAFETY: the caller provides an exclusive channel, two strings and a writable out pointer.
            let (Some(channel), Some(code), Some(name), Some(out)) = (unsafe {
                (
                    channel.as_mut(),
                    argument(code),
                    argument(name),
                    out.as_mut(),
                )
            }) else {
                return 1;
            };
            match channel.pair_code(&code, version, &name) {
                Ok(reply) => {
                    *out = c_string(&reply);
                    0
                }
                Err(PairError::Unavailable) => 1,
                Err(PairError::Refused) => 3,
            }
        },
        || 1,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn cf_phone_fingerprint(channel: *const Channel) -> *mut c_char {
    // SAFETY: the caller holds a live channel for the duration of the call.
    unsafe { channel.as_ref() }.map_or(ptr::null_mut(), |channel| {
        c_string(&hex::encode(channel.fingerprint))
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn cf_phone_exchange(
    channel: *mut Channel,
    line: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    guarded(
        || {
            // SAFETY: the caller provides an exclusive live channel, a string and a writable out pointer.
            let (Some(channel), Some(line), Some(out)) =
                (unsafe { (channel.as_mut(), argument(line), out.as_mut()) })
            else {
                return 1;
            };
            match channel.exchange(&line) {
                Ok(Some(reply)) => {
                    *out = c_string(&reply);
                    0
                }
                Ok(None) => 2,
                Err(_) => 1,
            }
        },
        || 1,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn cf_phone_close(channel: *mut Channel) {
    if !channel.is_null() {
        // SAFETY: this came from Box::into_raw in cf_phone_open and is being freed once.
        drop(unsafe { Box::from_raw(channel) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn cf_phone_proof(
    key: *const c_char,
    fingerprint: *const c_char,
    agent: u8,
) -> *mut c_char {
    guarded(
        || {
            // SAFETY: both arguments are valid C strings for the duration of the call.
            let (key, fingerprint) = unsafe { (argument(key), argument(fingerprint)) };
            let Some(key) = key.as_deref().and_then(hex32) else {
                return ptr::null_mut();
            };
            let Some(fingerprint) = fingerprint.as_deref().and_then(hex32) else {
                return ptr::null_mut();
            };
            let role = if agent == 0 { Role::Phone } else { Role::Agent };
            c_string(&hex::encode(qr_proof(&key, role, &fingerprint).as_ref()))
        },
        ptr::null_mut,
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::net::{Shutdown, TcpListener};
    use std::sync::mpsc;
    use std::thread::{self, JoinHandle};

    use rustls::pki_types::PrivatePkcs8KeyDer;
    use rustls::{ServerConfig, ServerConnection};

    use super::*;

    fn tls_peer(
        protocol: Option<&[u8]>,
        after_handshake: impl FnOnce(&mut StreamOwned<ServerConnection, TcpStream>) + Send + 'static,
    ) -> (u16, JoinHandle<()>) {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let key = PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());
        let mut config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&rustls::version::TLS13])
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(vec![cert.cert.der().clone()], key.into())
                .unwrap();
        config.alpn_protocols = protocol.map(<[u8]>::to_vec).into_iter().collect();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.set_read_timeout(Some(TIMEOUT)).unwrap();
            socket.set_write_timeout(Some(TIMEOUT)).unwrap();
            let mut server = ServerConnection::new(Arc::new(config)).unwrap();
            while server.is_handshaking() {
                if server.complete_io(&mut socket).is_err() {
                    return;
                }
            }
            after_handshake(&mut StreamOwned::new(server, socket));
        });
        (port, task)
    }

    #[test]
    fn an_explicit_rejection_or_a_completed_handshake_without_alpn_requires_update() {
        let (port, server) = tls_peer(Some(b"other-protocol"), |_| {});
        assert!(matches!(
            Channel::open("127.0.0.1", port, None),
            Err(OpenError::ComputerUpdate)
        ));
        server.join().unwrap();

        let (port, server) = tls_peer(None, |_| {});
        assert!(matches!(
            Channel::open("127.0.0.1", port, None),
            Err(OpenError::ComputerUpdate)
        ));
        server.join().unwrap();
    }

    #[test]
    fn an_interrupted_handshake_is_unreachable() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || drop(listener.accept().unwrap()));
        assert!(matches!(
            Channel::open("127.0.0.1", port, None),
            Err(OpenError::Unavailable)
        ));
        server.join().unwrap();
    }

    #[test]
    fn a_typed_peer_that_closes_before_replying_is_unreachable() {
        for clean in [false, true] {
            let (port, server) = tls_peer(Some(TYPED_CAPABILITY), move |wire| {
                let mut line = String::new();
                BufReader::new(&mut *wire).read_line(&mut line).unwrap();
                assert!(!line.is_empty());
                if clean {
                    wire.conn.send_close_notify();
                    wire.flush().unwrap();
                }
            });
            let mut client = match Channel::open("127.0.0.1", port, None) {
                Ok(client) => client,
                Err(_) => panic!("The typed TLS connection should open."),
            };
            assert!(matches!(
                client.pair_code("01234567", 1, "Test"),
                Err(PairError::Unavailable)
            ));
            server.join().unwrap();
        }
    }

    #[test]
    fn an_idle_exchange_gets_the_terminal_notice_after_the_peer_half_closes() {
        let (unpair, revoked) = mpsc::channel();
        let (notice_sent, read_notice) = mpsc::channel();
        let auth = json!({"v": 1, "token": "test credential", "watch_revocation": true});
        let ping = json!({"cmd": "ping"});
        let expected = [auth.to_string(), ping.to_string()];
        let (port, server) = tls_peer(Some(TYPED_CAPABILITY), move |wire| {
            let mut reader = BufReader::new(wire);
            let mut line = String::new();
            for request in expected {
                reader.read_line(&mut line).unwrap();
                assert_eq!(line.trim_end(), request);
                line.clear();
                writeln!(reader.get_mut(), "{}", json!({"ok": true})).unwrap();
                reader.get_mut().flush().unwrap();
            }

            revoked.recv_timeout(TIMEOUT).unwrap();
            let wire = reader.get_mut();
            writeln!(wire, "{}", json!({"revoked": true})).unwrap();
            wire.conn.send_close_notify();
            wire.flush().unwrap();
            wire.sock.shutdown(Shutdown::Write).unwrap();
            notice_sent.send(()).unwrap();

            reader.read_line(&mut line).unwrap();
            assert_eq!(line.trim_end(), json!({"cmd": "ping"}).to_string());
        });
        let mut client = Channel::open("127.0.0.1", port, None)
            .unwrap_or_else(|_| panic!("Expected TLS connection"));
        for request in [&auth, &ping] {
            let reply = client.exchange(&request.to_string()).unwrap().unwrap();
            assert_eq!(reply, json!({"ok": true}).to_string());
        }

        unpair.send(()).unwrap();
        read_notice.recv_timeout(TIMEOUT).unwrap();
        let reply = client.exchange(&ping.to_string()).unwrap().unwrap();
        assert_eq!(reply, json!({"revoked": true}).to_string());
        server.join().unwrap();
    }

    #[test]
    fn invalid_challenges_and_unverified_proofs_offer_retry() {
        for malformed in [Some("not hex"), Some("00"), None] {
            let (port, server) = tls_peer(Some(TYPED_CAPABILITY), move |wire| {
                let binding = wire
                    .conn
                    .export_keying_material([0; BINDING_LEN], EXPORTER_LABEL, None)
                    .unwrap();
                let mut line = String::new();
                BufReader::new(&mut *wire).read_line(&mut line).unwrap();
                let challenge = if let Some(text) = malformed {
                    text.to_owned()
                } else {
                    let request: Value = serde_json::from_str(&line).unwrap();
                    let bytes =
                        hex::decode(request["pair_code"]["request"].as_str().unwrap()).unwrap();
                    let offer = phone_pairing::CodeOffer::new(b"87654321").unwrap();
                    let (_, challenge) = offer.challenge(&bytes, &binding, 1).unwrap();
                    hex::encode(challenge)
                };
                writeln!(wire, "{}", json!({"challenge": challenge})).unwrap();
                wire.flush().unwrap();
            });
            let mut client = Channel::open("127.0.0.1", port, None)
                .unwrap_or_else(|_| panic!("Expected typed TLS"));
            assert!(matches!(
                client.pair_code("01234567", 1, "Test"),
                Err(PairError::Unavailable)
            ));
            server.join().unwrap();
        }
    }

    #[test]
    fn a_lost_final_reply_allows_retry_and_an_explicit_rejection_needs_a_fresh_code() {
        for rejected in [false, true] {
            let (port, server) = tls_peer(Some(TYPED_CAPABILITY), move |wire| {
                let binding = wire
                    .conn
                    .export_keying_material([0; BINDING_LEN], EXPORTER_LABEL, None)
                    .unwrap();
                let mut line = String::new();
                let mut reader = BufReader::new(wire);
                reader.read_line(&mut line).unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                let message =
                    hex::decode(request["pair_code"]["request"].as_str().unwrap()).unwrap();
                let offer = phone_pairing::CodeOffer::new(b"01234567").unwrap();
                let (confirmation, challenge) = offer.challenge(&message, &binding, 1).unwrap();
                writeln!(
                    reader.get_mut(),
                    "{}",
                    json!({"challenge": hex::encode(challenge)})
                )
                .unwrap();
                reader.get_mut().flush().unwrap();
                line.clear();
                reader.read_line(&mut line).unwrap();
                let reply: Value = serde_json::from_str(&line).unwrap();
                assert!(
                    confirmation
                        .verify(&hex::decode(reply["confirmation"].as_str().unwrap()).unwrap())
                );
                if rejected {
                    writeln!(reader.get_mut(), "{}", json!({"error": CODE_REJECTED})).unwrap();
                    reader.get_mut().flush().unwrap();
                }
            });
            let mut client = Channel::open("127.0.0.1", port, None)
                .unwrap_or_else(|_| panic!("Expected typed TLS"));
            let result = client.pair_code("01234567", 1, "Test");
            assert!(matches!(
                (rejected, result),
                (true, Err(PairError::Refused)) | (false, Err(PairError::Unavailable))
            ));
            server.join().unwrap();
        }
    }
}
