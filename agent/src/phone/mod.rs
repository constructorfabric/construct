//! The phone endpoint: how a phone on the same network talks to the agent.
//!
//! **Channel.** TLS 1.3 over TCP, with one self-signed certificate of the agent. The phone pins
//! its SHA-256 fingerprint from the QR offer or after authenticating a typed pairing. The
//! address just routes the connection, so `10.0.2.2` of an emulator or a hotspot address work.
//!
//! **Pairing.** The agent opens a pairing and shows a QR payload (address, fingerprint, a long
//! secret) and a short code to type. The phone connects and authenticates with the QR secret or
//! OPAQUE with the typed code. The agent keeps a hash of the issued token (see `paired`); the
//! phone keeps the token. A secret works once.
//!
//! **Talking.** Later the phone sends its token as the first line of a connection, then one JSON
//! line per command, and the agent answers one JSON line each. Every line is at most
//! [`MAX_LINE`] bytes.
//!
//! **Silence.** The agent answers only what it can authenticate. A wrong QR proof, an unknown
//! token, a typed-code request on a connection that did not negotiate the typed protocol, and a
//! line the agent does not expect get no reply: the agent just closes the connection. A typed-code
//! request for a pairing that is closed, used up or expired gets the fixed error
//! `pairing code rejected`, and one for an open pairing gets an OPAQUE challenge. Only a phone
//! that proved itself (a right QR proof, a verified typed confirmation or a known token) is told
//! that its version is too old. A phone with a token-authenticated session that sets
//! `watch_revocation` also gets a final `{"revoked": true}` line when it is unpaired while
//! connected; a phone that does not set it is closed without one.
//!
//! **Limits of a pairing.** One budget of [`pairing::MAX_FAILURES`] failed attempts covers the
//! whole pairing and every peer on the network: a wrong QR proof, and a typed challenge that
//! ends without a verified confirmation (the phone left, or its confirmation does not verify).
//! Challenges still waiting for their confirmation hold a part of the budget too, so a pairing
//! never hands out more guesses than it allows. When the budget is used the pairing closes and
//! the person opens a new one. This is on purpose, since it is what keeps a short code from
//! being guessed; it also means a peer on the network can stop a pairing.
//!
//! **Versions.** Both sides state a protocol number (`v`). The agent answers phones from
//! [`MIN_PROTOCOL`] to [`PROTOCOL`].
//!
//! Errors carry fixed texts; the secret, the codes, the tokens and the keys are in no log.

pub mod live;
pub mod paired;
pub mod pairing;
mod service;
pub mod sync;
mod tls;

use std::fmt;
use std::future::Future;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, broadcast};
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

use paired::Paired;
use pairing::Pairing;
pub use service::Service;
use tls::Identity;
pub use tls::fingerprint_of;

/// The protocol number this agent speaks.
pub const PROTOCOL: u64 = 1;
/// The oldest protocol number this agent still answers.
pub const MIN_PROTOCOL: u64 = 1;
pub use phone_pairing::MAX_LINE;
/// How long a pairing stays open when the caller gives no time.
pub const DEFAULT_PAIRING_TIME: Duration = Duration::from_secs(300);
/// The port the agent listens on for phones, so a paired phone finds it again after a restart.
pub const DEFAULT_PORT: u16 = 47821;

const MAX_CONNECTIONS: usize = 32;
const HANDSHAKE_TIME: Duration = Duration::from_secs(10);
const FIRST_LINE_TIME: Duration = Duration::from_secs(10);
const IDLE_TIME: Duration = Duration::from_secs(60);

/// Why the endpoint could not start or open a pairing. The texts are fixed on purpose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A file of the endpoint could not be read or written.
    Storage,
    /// The certificate or its key is not usable.
    Tls,
    /// The system gave no random bytes.
    Random,
    /// The port could not be opened.
    Bind,
    /// The computer has no address on a local network.
    NoNetwork,
}

impl Error {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Error::Storage => "phone endpoint files could not be used",
            Error::Tls => "phone endpoint certificate is not usable",
            Error::Random => "no random bytes available",
            Error::Bind => "phone endpoint port could not be opened",
            Error::NoNetwork => "no local network address found",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::error::Error for Error {}

/// How a phone's protocol number fits the agent's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    Ok,
    /// The phone is older than [`MIN_PROTOCOL`]: it must update.
    PhoneTooOld,
    /// The phone is newer than [`PROTOCOL`]: the agent must update.
    AgentTooOld,
}

/// Compare a phone's protocol number with the agent's.
#[must_use]
pub fn check_version(phone: u64) -> Version {
    if phone < MIN_PROTOCOL {
        Version::PhoneTooOld
    } else if phone > PROTOCOL {
        Version::AgentTooOld
    } else {
        Version::Ok
    }
}

/// The address of this computer on its local network: the address of the interface that has
/// the default route, which on a phone's hotspot is the hotspot's. Nothing is sent: the lookup
/// only asks the system which interface it would use. This is the one supported case: with no
/// default route (a network that is offline) it fails, and under a VPN that takes all traffic it
/// gives the tunnel's address. The caller then names the address itself, as the example's
/// `--host` does.
///
/// # Errors
/// Fails when the computer has no route to any network.
pub fn lan_address() -> Result<IpAddr, Error> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).map_err(|_| Error::NoNetwork)?;
    socket
        .connect((Ipv4Addr::new(8, 8, 8, 8), 80))
        .map_err(|_| Error::NoNetwork)?;
    let ip = socket.local_addr().map_err(|_| Error::NoNetwork)?.ip();
    if ip.is_unspecified() || ip.is_loopback() {
        return Err(Error::NoNetwork);
    }
    Ok(ip)
}

/// What the endpoint needs to start.
pub struct Config {
    /// The directory of the certificate, its key and the paired phones.
    pub dir: PathBuf,
    /// The address to listen on; port 0 takes any free port.
    pub bind: SocketAddr,
}

/// What the person sees when a pairing is open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingOffer {
    /// `host:port` the phone connects to.
    pub address: String,
    /// The text of the QR code: `construct-pair://<address>?v=<protocol>&fp=<hex>&s=<hex>`.
    pub payload: String,
    /// The short code to type, digits only.
    pub code: String,
    /// The fingerprint of the certificate, hex.
    pub fingerprint: String,
}

struct State {
    sync: std::sync::Mutex<Option<Arc<sync::Engine>>>,
    identity: Identity,
    paired: Arc<Paired>,
    pairing: Pairing,
}

/// The listener for phones.
pub struct Endpoint {
    state: Arc<State>,
    local: SocketAddr,
    task: JoinHandle<()>,
}

impl Endpoint {
    /// Open the port and start answering.
    ///
    /// # Errors
    /// Fails when the files cannot be used or the port cannot be opened.
    pub async fn start(config: Config) -> Result<Self, Error> {
        let paired = Arc::new(Paired::load(&config.dir)?);
        Self::start_with(config, paired).await
    }

    /// Open the port and start answering the phones in `paired`.
    ///
    /// # Errors
    /// Fails when the files cannot be used or the port cannot be opened.
    pub async fn start_with(config: Config, paired: Arc<Paired>) -> Result<Self, Error> {
        let identity = Identity::load_or_create(&config.dir)?;
        let acceptor = TlsAcceptor::from(identity.server_config()?);
        let listener = TcpListener::bind(config.bind)
            .await
            .map_err(|_| Error::Bind)?;
        let local = listener.local_addr().map_err(|_| Error::Bind)?;
        let state = Arc::new(State {
            sync: std::sync::Mutex::new(None),
            identity,
            paired,
            pairing: Pairing::default(),
        });
        let task = tokio::spawn(accept_loop(listener, acceptor, Arc::clone(&state)));
        Ok(Self { state, local, task })
    }

    /// The address the endpoint listens on.
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// Serve the sync commands from `engine`; without one they answer an error.
    pub fn set_sync(&self, engine: Option<Arc<sync::Engine>>) {
        *crate::sync::lock(&self.state.sync) = engine;
    }

    /// Tell every paired phone that everything was erased (see [`paired::Paired::renew_reset`]).
    ///
    /// # Errors
    /// Fails when the phones' file cannot be written.
    pub fn reset_phones(&self) -> Result<(), Error> {
        self.state.paired.renew_reset()
    }

    /// How many phones have paired.
    #[must_use]
    pub fn paired_count(&self) -> usize {
        self.state.paired.len()
    }

    /// Open a pairing for `ttl`, in place of any open one. `host` is the address the phone
    /// should connect to, usually [`lan_address`].
    ///
    /// # Errors
    /// Fails when the system gives no random bytes.
    pub fn open_pairing(&self, host: IpAddr, ttl: Duration) -> Result<PairingOffer, Error> {
        let offer = self.state.pairing.open(ttl)?;
        let address = SocketAddr::new(host, self.local.port()).to_string();
        let fingerprint = hex::encode(self.state.identity.fingerprint());
        let payload = format!(
            "construct-pair://{address}?v={PROTOCOL}&fp={fingerprint}&s={}",
            offer.secret
        );
        Ok(PairingOffer {
            address,
            payload,
            code: offer.code,
            fingerprint,
        })
    }

    /// Close the open pairing, if any.
    pub fn close_pairing(&self) {
        self.state.pairing.close();
    }

    /// Whether the current offer can still be used.
    #[must_use]
    pub fn pairing_is_open(&self) -> bool {
        self.state.pairing.is_open()
    }

    /// Whether a pairing is open, or a phone that proved itself is still being stored.
    #[must_use]
    pub fn pairing_is_busy(&self) -> bool {
        self.state.pairing.is_busy()
    }

    /// A wait that ends when no pairing is open and no phone is being stored. It does not borrow
    /// the endpoint, so the caller may drop or stop it meanwhile.
    pub fn wait_pairing_over(&self) -> impl Future<Output = ()> + Send + 'static {
        let state = Arc::clone(&self.state);
        async move { state.pairing.wait_idle().await }
    }

    /// Stop listening and wait until the port is closed. Connections already open stay until
    /// their phone is unpaired or they go idle; a new one is not accepted.
    pub async fn stop(self) {
        self.task.abort();
        let _ = self.task.await;
    }
}

async fn accept_loop(listener: TcpListener, acceptor: TlsAcceptor, state: Arc<State>) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            // A failed accept (no file descriptors left, say) must not spin.
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        };
        // Over the limit, the connection is dropped without a byte.
        let Ok(slot) = Arc::clone(&slots).try_acquire_owned() else {
            continue;
        };
        let acceptor = acceptor.clone();
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            let _slot = slot;
            let _ = serve(stream, acceptor, &state).await;
        });
    }
}

/// One line of at most [`MAX_LINE`] bytes, without its newline; `None` when the peer closed, the
/// line is too long or not UTF-8.
async fn read_line<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> Option<String> {
    let mut buf = Vec::new();
    let mut limited = reader.take(MAX_LINE as u64 + 1);
    limited.read_until(b'\n', &mut buf).await.ok()?;
    if buf.pop() != Some(b'\n') || buf.len() > MAX_LINE {
        return None;
    }
    String::from_utf8(buf).ok()
}

async fn write_line<W: AsyncWrite + Unpin>(writer: &mut W, value: &Value) -> io::Result<()> {
    let mut line = value.to_string();
    line.push('\n');
    writer.write_all(line.as_bytes()).await?;
    writer.flush().await
}

/// Take one connection through the handshake, the first line and the commands. An `Err` or
/// `None` anywhere closes the connection without an answer.
async fn serve(stream: TcpStream, acceptor: TlsAcceptor, state: &State) -> Option<()> {
    let tls = tokio::time::timeout(HANDSHAKE_TIME, acceptor.accept(stream))
        .await
        .ok()?
        .ok()?;
    let typed = tls.get_ref().1.alpn_protocol() == Some(phone_pairing::TYPED_CAPABILITY);
    let exporter = tls
        .get_ref()
        .1
        .export_keying_material(
            [0; phone_pairing::BINDING_LEN],
            phone_pairing::EXPORTER_LABEL,
            None,
        )
        .ok()?;
    let (reader, mut writer) = tokio::io::split(tls);
    let mut reader = BufReader::new(reader);
    let first = tokio::time::timeout(FIRST_LINE_TIME, read_line(&mut reader))
        .await
        .ok()??;
    let hello: Value = serde_json::from_str(&first).ok()?;
    let offered_version = hello.get("v")?.as_u64()?;
    let version = check_version(offered_version);

    if let Some(pair) = hello.get("pair") {
        let proof = pair.get("proof")?.as_str()?;
        let name = pair.get("name").and_then(Value::as_str).unwrap_or_default();
        // The certificate the phone saw is this one: it is what the proof is bound to.
        let fingerprint = state.identity.fingerprint();
        // Held until the phone is stored: until then the pairing is gone and the list is empty,
        // and the service must not read that as "nobody to listen for".
        let _redeeming = state.pairing.redeeming();
        return match state.pairing.redeem(proof, &fingerprint, version) {
            pairing::Redeem::Granted(agent_proof) => {
                let mut reply = save_pairing(state, name)?;
                reply["agent_proof"] = json!(agent_proof);
                write_line(&mut writer, &reply).await.ok()
            }
            pairing::Redeem::Version(version, agent_proof) => {
                let mut reply = version_reply(version);
                reply["agent_proof"] = json!(agent_proof);
                write_line(&mut writer, &reply).await.ok()
            }
            pairing::Redeem::Refused => None,
        };
    }

    if let Some(pair) = hello.get("pair_code") {
        if !typed {
            return None;
        }
        let request = hex::decode(pair.get("request")?.as_str()?).ok()?;
        return pair_by_code(
            &mut reader,
            &mut writer,
            state,
            &exporter,
            &request,
            offered_version,
        )
        .await;
    }

    let token = hello.get("token")?.as_str()?;
    let watch = hello.get("watch_revocation") == Some(&Value::Bool(true));
    // Hear the unpairing before looking the phone up, so none falls between the two.
    let mut revoked = state.paired.subscribe();
    let id = state.paired.id_of(token)?;
    if version != Version::Ok {
        return write_line(&mut writer, &version_reply(version)).await.ok();
    }
    write_line(&mut writer, &json!({"v": PROTOCOL, "ok": true}))
        .await
        .ok()?;
    let result = commands(&mut reader, &mut writer, state, &id, &mut revoked).await;
    if watch && !state.paired.has_id(&id) {
        // Only a session that passed token authentication receives the final notification.
        write_line(&mut writer, &json!({"revoked": true}))
            .await
            .ok()?;
        writer.shutdown().await.ok()?;
        // Closing a socket with an unread ping can reset the connection before the phone
        // reads the notice. Allow the normal session idle window for a delayed phone, while
        // the half-closed socket can no longer answer it.
        let mut remaining = reader.take(MAX_LINE as u64);
        let _ = tokio::time::timeout(
            IDLE_TIME,
            tokio::io::copy(&mut remaining, &mut tokio::io::sink()),
        )
        .await;
    }
    result
}

async fn pair_by_code<R, W>(
    reader: &mut BufReader<R>,
    writer: &mut W,
    state: &State,
    exporter: &[u8; phone_pairing::BINDING_LEN],
    request: &[u8],
    offered_version: u64,
) -> Option<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let Some((attempt, challenge)) = state.pairing.start_code(request, exporter, offered_version)
    else {
        return write_line(writer, &json!({"error": phone_pairing::CODE_REJECTED}))
            .await
            .ok();
    };
    write_line(writer, &json!({"challenge": hex::encode(challenge)}))
        .await
        .ok()?;
    let confirmation = tokio::time::timeout(FIRST_LINE_TIME, read_line(reader))
        .await
        .ok()??;
    let confirmation: Value = serde_json::from_str(&confirmation).ok()?;
    let name = confirmation
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let confirmation = hex::decode(confirmation.get("confirmation")?.as_str()?).ok()?;
    let _redeeming = state.pairing.redeeming();
    let version = check_version(offered_version);
    if !attempt.finish(&confirmation, version) {
        return write_line(writer, &json!({"error": phone_pairing::CODE_REJECTED}))
            .await
            .ok();
    }
    let reply = if version == Version::Ok {
        save_pairing(state, name)?
    } else {
        version_reply(version)
    };
    write_line(writer, &reply).await.ok()
}

fn save_pairing(state: &State, name: &str) -> Option<Value> {
    // The offer is spent. If it cannot be saved, open a fresh offer to retry.
    let Ok(token) = state.paired.add(name) else {
        eprintln!("phone endpoint: a paired phone could not be saved");
        return None;
    };
    Some(json!({"v": PROTOCOL, "paired": true, "token": token}))
}

/// Completes when the phone `id` is unpaired. A receiver that fell behind may have missed it, so
/// that ends the wait too: the channel closes and a phone that is still paired connects again.
async fn unpaired(revoked: &mut broadcast::Receiver<String>, id: &str) {
    loop {
        match revoked.recv().await {
            Ok(gone) if gone != id => {}
            Ok(_) | Err(_) => return,
        }
    }
}

fn version_reply(version: Version) -> Value {
    match version {
        Version::AgentTooOld => json!({"v": PROTOCOL, "error": "agent update needed"}),
        _ => json!({"v": PROTOCOL, "error": "update needed", "min_version": MIN_PROTOCOL}),
    }
}

/// The commands of a paired phone, one JSON line each.
///
/// The channel ends the moment the phone is unpaired, and no command is answered after that.
async fn commands<R, W>(
    reader: &mut BufReader<R>,
    writer: &mut W,
    state: &State,
    id: &str,
    revoked: &mut broadcast::Receiver<String>,
) -> Option<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let paired = &state.paired;
    loop {
        let line = tokio::select! {
            line = tokio::time::timeout(IDLE_TIME, read_line(reader)) => line.ok()??,
            () = unpaired(revoked, id) => return None,
        };
        // `select!` may take the line when the unpairing is ready too: check before answering.
        if !paired.has_id(id) {
            return None;
        }
        let request: Value = serde_json::from_str(&line).ok()?;
        let reply = match request.get("cmd").and_then(Value::as_str) {
            Some("ping") => json!({
                "ok": true,
                "agent": "construct-agent",
                "agent_version": env!("CARGO_PKG_VERSION"),
            }),
            Some(command) if sync::handles(command) => {
                // Read before the answer: a check that spans an erase then carries the old marker, and the
                // new one shows at the next check; never the new marker with the old data's tokens.
                let marker = (command == "sync-check")
                    .then(|| paired.reset_marker())
                    .flatten();
                let engine = crate::sync::lock(&state.sync).clone();
                let mut reply = match engine {
                    // Counting takes seconds: not on a thread of the runtime.
                    Some(engine) => {
                        let request = request.clone();
                        tokio::task::spawn_blocking(move || engine.answer(&request))
                            .await
                            .unwrap_or_else(|_| json!({"error": "sync unavailable"}))
                    }
                    None => json!({"error": "sync unavailable"}),
                };
                if let (Some(marker), Some(object)) = (marker, reply.as_object_mut())
                    && object.get("ok") == Some(&Value::Bool(true))
                {
                    object.insert("reset".to_owned(), Value::String(marker));
                }
                reply
            }
            _ => json!({"error": "unknown command"}),
        };
        // The phone may have been unpaired while the answer was counted.
        if !paired.has_id(id) {
            return None;
        }
        write_line(writer, &reply).await.ok()?;
    }
}
