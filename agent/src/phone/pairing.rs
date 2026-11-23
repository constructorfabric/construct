//! The one-time secret of a pairing.
//!
//! A pairing has two forms of one secret: a long random one for the QR code and a short numeric
//! one to type. The QR proof binds the long secret to the certificate fingerprint. The typed
//! code uses OPAQUE bound to the TLS connection. The first successful redemption closes the
//! offer. Three failed attempts or expiry also close it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use phone_pairing::{CodeConfirmation, CodeOffer, Role, qr_proof, verify_qr};
use tokio::sync::Notify;

use super::{Error, Version};

/// The digits of the short code.
pub const CODE_DIGITS: usize = 8;
/// The wrong proofs a pairing takes before it closes.
pub const MAX_FAILURES: u8 = 3;

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
    Version(Version, String),
    /// No open pairing, or the proof is wrong.
    Refused,
}

struct Open {
    secret: [u8; 32],
    code: Arc<CodeOffer>,
    expires: Instant,
    failures: u8,
    pending: u8,
}

pub(super) struct CodeAttempt<'a> {
    pairing: &'a Pairing,
    offer: Arc<CodeOffer>,
    confirmation: Option<CodeConfirmation>,
    settled: bool,
}

impl CodeAttempt<'_> {
    pub(super) fn finish(mut self, confirmation: &[u8], version: Version) -> bool {
        if !self
            .confirmation
            .take()
            .is_some_and(|state| state.verify(confirmation))
        {
            return false;
        }
        let mut guard = self.pairing.lock();
        let Some(open) = guard
            .as_mut()
            .filter(|open| Arc::ptr_eq(&open.code, &self.offer))
        else {
            return false;
        };
        if open.expires <= Instant::now() {
            return false;
        }
        open.pending -= 1;
        if version == Version::Ok {
            *guard = None;
            self.pairing.ended.notify_waiters();
        }
        self.settled = true;
        true
    }
}

impl Drop for CodeAttempt<'_> {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        let mut guard = self.pairing.lock();
        if let Some(open) = guard
            .as_mut()
            .filter(|open| Arc::ptr_eq(&open.code, &self.offer))
        {
            open.pending -= 1;
            open.failures += 1;
            if open.failures == MAX_FAILURES {
                *guard = None;
                self.pairing.ended.notify_waiters();
            }
        }
    }
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
        let registration = CodeOffer::new(code.as_bytes()).ok_or(Error::Random)?;
        let offer = Offer {
            secret: hex::encode(secret),
            code,
        };
        *self.lock() = Some(Open {
            secret,
            code: Arc::new(registration),
            expires: Instant::now() + ttl,
            failures: 0,
            pending: 0,
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
        if !verify_qr(&open.secret, Role::Phone, fingerprint, &given) {
            if open.failures + open.pending < MAX_FAILURES {
                open.failures += 1;
                if open.failures == MAX_FAILURES {
                    *guard = None;
                    self.ended.notify_waiters();
                }
            }
            return Redeem::Refused;
        }
        let agent_proof = hex::encode(qr_proof(&open.secret, Role::Agent, fingerprint).as_ref());
        if version != Version::Ok {
            return Redeem::Version(version, agent_proof);
        }
        *guard = None;
        self.ended.notify_waiters();
        Redeem::Granted(agent_proof)
    }

    pub(super) fn start_code(
        &self,
        request: &[u8],
        binding: &[u8; phone_pairing::BINDING_LEN],
        version: u64,
    ) -> Option<(CodeAttempt<'_>, Vec<u8>)> {
        let offer = {
            let mut guard = self.lock();
            let open = guard.as_mut()?;
            if open.expires <= Instant::now() || open.failures + open.pending >= MAX_FAILURES {
                return None;
            }
            open.pending += 1;
            Arc::clone(&open.code)
        };
        // Receiving a challenge permits a guess even if the phone sends no confirmation.
        let mut attempt = CodeAttempt {
            pairing: self,
            offer,
            confirmation: None,
            settled: false,
        };
        let (confirmation, challenge) = attempt.offer.challenge(request, binding, version)?;
        attempt.confirmation = Some(confirmation);
        Some((attempt, challenge))
    }
}
