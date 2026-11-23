//! When the agent listens for phones, and what the app may ask about them.
//!
//! The agent opens no port before the first pairing. It listens while a pairing is open, and
//! from the first phone that paired on, also after a restart, until the last phone is unpaired.
//! The app asks through the local protocol (see `ipc`): `phone-pairing-open` answers the QR
//! payload and the short code, `phone-pairing-status` checks whether it is still usable, and
//! `phone-pairing-close` closes it. `phone-list` answers the phones and `phone-unpair` removes
//! one. No answer holds a token, a hash of it, or the secret of a pairing beyond the payload
//! and the code that the pairing screen shows.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;
use tokio::runtime::Handle;
use tokio::sync::Mutex;

use super::paired::{Paired, PhoneInfo};
use super::{Config, DEFAULT_PAIRING_TIME, Endpoint, Error, PairingOffer, lan_address};
use crate::ipc::error_response;

struct Inner {
    dir: PathBuf,
    bind: SocketAddr,
    /// The address to put in the QR payload; the address of this computer when `None`, which is
    /// right unless the computer has several networks and the caller knows which one the phone is on.
    host: Option<IpAddr>,
    paired: Arc<Paired>,
    endpoint: Mutex<Option<Endpoint>>,
    handle: Handle,
}

/// The argument of `phone-unpair`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Unpair {
    id: String,
    #[serde(default)]
    ack_only: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    after: Option<String>,
}

// Eight entries of up to 64 Unicode characters, generated ids and timestamps fit well within
// the desktop socket's 4 KiB reply budget, even at four bytes per character.
const PAGE_SIZE: usize = 8;

/// The phone endpoint, started and stopped as the phones and the pairing need it.
#[derive(Clone)]
pub struct Service(Arc<Inner>);

impl Service {
    /// Read the paired phones in `dir` and listen on `bind` when there are any.
    ///
    /// # Errors
    /// Fails when the phones' file cannot be read. When the port cannot be opened for the phones
    /// that are paired, the service still starts, so the app can list and unpair them; the next
    /// pairing tries the port again.
    pub async fn start(
        dir: PathBuf,
        bind: SocketAddr,
        host: Option<IpAddr>,
    ) -> Result<Self, Error> {
        let paired = Arc::new(Paired::load(&dir)?);
        let service = Self(Arc::new(Inner {
            dir,
            bind,
            host,
            paired,
            endpoint: Mutex::new(None),
            handle: Handle::current(),
        }));
        if !service.0.paired.is_empty() {
            let mut slot = service.0.endpoint.lock().await;
            if let Err(e) = service.listen(&mut slot).await {
                eprintln!("phones: {e}");
            }
        }
        Ok(service)
    }

    async fn listen(&self, slot: &mut Option<Endpoint>) -> Result<(), Error> {
        if slot.is_none() {
            let config = Config {
                dir: self.0.dir.clone(),
                bind: self.0.bind,
            };
            *slot = Some(Endpoint::start_with(config, Arc::clone(&self.0.paired)).await?);
        }
        Ok(())
    }

    /// Open a pairing, and the port with it when it is closed.
    ///
    /// # Errors
    /// Fails when there is no network address, the port cannot be opened or the system gives no
    /// random bytes.
    pub async fn open_pairing(&self, ttl: Duration) -> Result<PairingOffer, Error> {
        let host = match self.0.host {
            Some(host) => host,
            None => lan_address()?,
        };
        let offer = {
            let mut slot = self.0.endpoint.lock().await;
            self.listen(&mut slot).await?;
            let endpoint = slot.as_ref().ok_or(Error::Bind)?;
            let offer = endpoint.open_pairing(host, ttl)?;
            let over = endpoint.wait_pairing_over();
            let service = self.clone();
            // Closes the port when the pairing ends with no phone paired.
            tokio::spawn(async move {
                over.await;
                service.settle().await;
            });
            offer
        };
        Ok(offer)
    }

    /// Close the pairing; the port closes too when no phone is paired.
    pub async fn close_pairing(&self) {
        if let Some(endpoint) = self.0.endpoint.lock().await.as_ref() {
            endpoint.close_pairing();
        }
        self.settle().await;
    }

    /// The paired phones.
    #[must_use]
    pub fn phones(&self) -> Vec<PhoneInfo> {
        self.0.paired.list()
    }

    /// Unpair the phone with this id: its open channels end at once. The port closes when it was
    /// the last phone and no pairing is open. `false` when there is no such phone.
    ///
    /// # Errors
    /// Fails when the phones' file cannot be written; then the phone stays paired.
    pub async fn unpair(&self, id: &str) -> Result<bool, Error> {
        let removed = self.0.paired.remove(id)?;
        self.settle().await;
        Ok(removed)
    }

    /// Stop listening, whatever the phones.
    pub async fn stop(&self) {
        if let Some(endpoint) = self.0.endpoint.lock().await.take() {
            endpoint.stop().await;
        }
    }

    /// Close the port when no phone is paired and no pairing is open.
    async fn settle(&self) {
        let mut slot = self.0.endpoint.lock().await;
        let idle = slot
            .as_ref()
            // Check pairing first: a redemption can finish and store a phone between checks.
            .is_some_and(|e| !e.pairing_is_busy() && self.0.paired.is_empty());
        if idle && let Some(endpoint) = slot.take() {
            endpoint.stop().await;
        }
    }

    /// The commands of the local protocol this service answers.
    pub const COMMANDS: [&'static str; 5] = [
        "phone-pairing-open",
        "phone-pairing-close",
        "phone-pairing-status",
        "phone-list",
        "phone-unpair",
    ];

    /// Whether `command` is one of [`Service::COMMANDS`].
    #[must_use]
    pub fn handles(command: &str) -> bool {
        Self::COMMANDS.contains(&command)
    }

    /// Answer a phone command of the local protocol, as one JSON line. Called from
    /// the thread of a connection, not from the runtime.
    #[must_use]
    pub fn command(&self, command: &str, argument: &str) -> String {
        self.0.handle.block_on(self.answer(command, argument))
    }

    async fn answer(&self, command: &str, argument: &str) -> String {
        match command {
            "phone-pairing-open" if argument.is_empty() => {
                match self.open_pairing(DEFAULT_PAIRING_TIME).await {
                    Ok(offer) => json!({
                        "payload": offer.payload,
                        "code": offer.code,
                        "address": offer.address,
                        "expires_in": DEFAULT_PAIRING_TIME.as_secs(),
                    })
                    .to_string(),
                    Err(e) => error_response(e.as_str()),
                }
            }
            "phone-pairing-close" if argument.is_empty() => {
                self.close_pairing().await;
                json!({"ok": true}).to_string()
            }
            "phone-pairing-status" if argument.is_empty() => {
                let slot = self.0.endpoint.lock().await;
                json!({"active": slot.as_ref().is_some_and(Endpoint::pairing_is_open)}).to_string()
            }
            "phone-list" if argument.is_empty() => self.list_response(),
            "phone-list" => {
                let Ok(Page { after }) = serde_json::from_str(argument) else {
                    return error_response("invalid phone request");
                };
                let mut phones = self.0.paired.page(after.as_deref(), PAGE_SIZE + 1);
                let next = if phones.len() > PAGE_SIZE {
                    phones.truncate(PAGE_SIZE);
                    Some(phones[PAGE_SIZE - 1].id.as_str())
                } else {
                    None
                };
                json!({"phones": phones, "next": next}).to_string()
            }
            "phone-unpair" => {
                let Ok(Unpair { id, ack_only }) = serde_json::from_str(argument) else {
                    return error_response("invalid phone request");
                };
                match self.unpair(&id).await {
                    Ok(true) if ack_only => json!({"ok": true}).to_string(),
                    Ok(true) => self.list_response(),
                    Ok(false) => error_response("no such phone"),
                    Err(e) => error_response(e.as_str()),
                }
            }
            _ => error_response("invalid phone request"),
        }
    }

    fn list_response(&self) -> String {
        json!({"phones": self.phones()}).to_string()
    }
}
