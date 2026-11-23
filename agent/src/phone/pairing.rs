//! The one-time secret of a pairing.
//!
//! A pairing has two forms of one secret: a long random one for the QR code and a short numeric
//! one to type. Either opens the pairing, once; the first use closes it. Neither is ever sent:
//! the phone sends a proof, a keyed hash of the certificate fingerprint it saw, so a computer
//! that is not this one (a different certificate) cannot pass, and the agent answers with a
//! proof of its own, so the phone knows the agent had the secret too. After three wrong proofs,
//! or when the time is up, the pairing closes and the agent must open a new one.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use tokio::sync::Notify;

use super::{Error, Version};

/// The digits of the short code.
pub const CODE_DIGITS: usize = 8;
/// The wrong proofs a pairing takes before it closes.
pub const MAX_FAILURES: u8 = 3;

type HmacSha256 = Hmac<Sha256>;

/// Who made a proof; the two proofs of a pairing differ so one cannot be replayed as the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Phone,
    Agent,
}

fn mac(secret: &[u8], role: Role, fingerprint: &[u8; 32]) -> HmacSha256 {
    // An HMAC takes a key of any length, so this never fails.
    let mut mac = HmacSha256::new_from_slice(secret).unwrap_or_else(|_| unreachable!());
    mac.update(b"construct-pair\0");
    mac.update(match role {
        Role::Phone => b"phone",
        Role::Agent => b"agent",
    });
    mac.update(fingerprint);
    mac
}

/// The proof of `role`: the hex of an HMAC-SHA-256 keyed with the secret over the fixed text
/// `construct-pair`, a zero byte, the role (`phone` or `agent`) and the 32 bytes of the SHA-256
/// fingerprint of the agent's certificate as the phone sees it. For the long secret the key is
/// its 32 bytes; for the short code it is the digits as ASCII.
#[must_use]
pub fn proof(secret: &[u8], role: Role, fingerprint: &[u8; 32]) -> String {
    hex::encode(mac(secret, role, fingerprint).finalize().into_bytes())
}

/// A pairing that is open.
pub struct Offer {
    /// The long secret, hex, for the QR code.
    pub secret: String,
    /// The short code to type: digits only.
    pub code: String,
}

/// What the agent made of a proof.
#[derive(Debug, PartialEq, Eq)]
pub enum Redeem {
    /// The proof is right: the pairing is used up. The agent's own proof is attached.
    Granted(String),
    /// The proof is right but the versions do not fit; the pairing stays open.
    Version(Version),
    /// No open pairing, or the proof is wrong.
    Refused,
}

struct Open {
    secret: [u8; 32],
    code: String,
    expires: Instant,
    failures: u8,
}

/// The pairing the agent has open, if any.
#[derive(Default)]
pub struct Pairing {
    open: Mutex<Option<Open>>,
    /// Phones whose proof is being checked or whose token is being stored.
    redeeming: AtomicUsize,
    /// Woken when the pairing ends or a redeeming phone is done.
    ended: Notify,
}

/// Marks a phone as redeeming; see [`Pairing::redeeming`].
pub struct Redeeming<'a>(&'a Pairing);

impl Drop for Redeeming<'_> {
    fn drop(&mut self) {
        self.0.redeeming.fetch_sub(1, Ordering::SeqCst);
        self.0.ended.notify_waiters();
    }
}

fn random_code() -> Result<String, Error> {
    // Take 32 random bits and drop the top of the range, so every code is equally likely.
    const LIMIT: u32 = 42 * 100_000_000;
    loop {
        let mut bytes = [0u8; 4];
        getrandom::fill(&mut bytes).map_err(|_| Error::Random)?;
        let n = u32::from_le_bytes(bytes);
        if n < LIMIT {
            return Ok(format!("{:0width$}", n % 100_000_000, width = CODE_DIGITS));
        }
    }
}

impl Pairing {
    fn lock(&self) -> MutexGuard<'_, Option<Open>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Open a pairing for `ttl`, in place of any that is open.
    ///
    /// # Errors
    /// Fails when the system gives no random bytes.
    pub fn open(&self, ttl: Duration) -> Result<Offer, Error> {
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).map_err(|_| Error::Random)?;
        let code = random_code()?;
        let offer = Offer {
            secret: hex::encode(secret),
            code: code.clone(),
        };
        *self.lock() = Some(Open {
            secret,
            code,
            expires: Instant::now() + ttl,
            failures: 0,
        });
        Ok(offer)
    }

    /// Close the pairing; its secrets work no more.
    pub fn close(&self) {
        *self.lock() = None;
        self.ended.notify_waiters();
    }

    /// Count a phone as redeeming until the guard is dropped. A caller takes it before
    /// [`Pairing::redeem`] and keeps it until the phone is stored, so that no one sees the
    /// pairing closed and nothing paired in between.
    pub fn redeeming(&self) -> Redeeming<'_> {
        self.redeeming.fetch_add(1, Ordering::SeqCst);
        Redeeming(self)
    }

    /// Whether a pairing is open or a phone is still being stored.
    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.is_open() || self.redeeming.load(Ordering::SeqCst) > 0
    }

    /// Wait until the pairing is over: used up, closed, out of time or out of tries, and no
    /// phone left to store.
    pub async fn wait_idle(&self) {
        loop {
            let ended = self.ended.notified();
            tokio::pin!(ended);
            ended.as_mut().enable();
            if !self.is_busy() {
                return;
            }
            let left = self
                .lock()
                .as_ref()
                .map(|o| o.expires.saturating_duration_since(Instant::now()));
            match left {
                Some(left) => {
                    let _ = tokio::time::timeout(left, ended).await;
                }
                None => ended.await,
            }
        }
    }

    /// Whether a pairing is open and not out of time.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.lock()
            .as_ref()
            .is_some_and(|o| o.expires > Instant::now())
    }

    /// Check the phone's proof, under one lock, so two phones cannot both use the secret.
    #[must_use]
    pub fn redeem(&self, proof_hex: &str, fingerprint: &[u8; 32], version: Version) -> Redeem {
        let mut guard = self.lock();
        let Some(open) = guard.as_mut() else {
            return Redeem::Refused;
        };
        if open.expires <= Instant::now() {
            *guard = None;
            self.ended.notify_waiters();
            return Redeem::Refused;
        }
        let given = hex::decode(proof_hex).unwrap_or_default();
        let candidates: [&[u8]; 2] = [&open.secret, open.code.as_bytes()];
        let matched = candidates.into_iter().find(|key| {
            mac(key, Role::Phone, fingerprint)
                .verify_slice(&given)
                .is_ok()
        });
        let Some(key) = matched else {
            open.failures += 1;
            if open.failures >= MAX_FAILURES {
                *guard = None;
                self.ended.notify_waiters();
            }
            return Redeem::Refused;
        };
        if version != Version::Ok {
            return Redeem::Version(version);
        }
        let agent_proof = proof(key, Role::Agent, fingerprint);
        *guard = None;
        self.ended.notify_waiters();
        Redeem::Granted(agent_proof)
    }
}
