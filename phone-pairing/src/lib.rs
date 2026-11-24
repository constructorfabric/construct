//! Pairing authentication for a high-entropy QR secret or a short typed code.

use opaque_ke::ciphersuite::CipherSuite;
use opaque_ke::generic_array::typenum::Unsigned;
use opaque_ke::{
    ClientLogin, ClientLoginFinishParameters, ClientRegistration,
    ClientRegistrationFinishParameters, CredentialFinalization, CredentialRequest,
    CredentialResponse, CredentialResponseLen, Identifiers, ServerLogin, ServerLoginParameters,
    ServerRegistration, ServerSetup,
};
use rand::rngs::OsRng;
use ring::hmac;

/// TLS 1.3 channel binding (RFC 9266).
pub const EXPORTER_LABEL: &[u8] = b"EXPORTER-Channel-Binding";
pub const BINDING_LEN: usize = 32;
/// TLS ALPN identifier for typed pairing with OPAQUE. The `v1` here and in the context label
/// name this construction and stay as they are; the protocol version the two sides negotiate is
/// the `version` argument mixed into every exchange.
pub const TYPED_CAPABILITY: &[u8] = b"construct-pair-code/opaque-rfc9807-v1";
/// The error text a computer sends for any rejected typed code: a wrong code, an offer that is
/// closed or used up, or a confirmation that does not verify. A phone that reads it asks for a
/// fresh code.
pub const CODE_REJECTED: &str = "pairing code rejected";

const OFFER_ID: &[u8] = b"construct-phone-pair-code";
const CONTEXT_LABEL: &[u8] = b"construct-phone-pair-code/opaque-rfc9807/v1\0";

struct Suite;

impl CipherSuite for Suite {
    type OprfCs = opaque_ke::Ristretto255;
    type KeyExchange = opaque_ke::TripleDh<opaque_ke::Ristretto255, sha2::Sha512>;
    // The per-offer server setup and registration live only in memory until the offer closes.
    type Ksf = opaque_ke::ksf::Identity;
}

fn context(binding: &[u8; BINDING_LEN], version: u64) -> Vec<u8> {
    [CONTEXT_LABEL, binding, &version.to_be_bytes()].concat()
}

/// One in-memory OPAQUE registration for a single open pairing offer.
///
/// The offer does not count attempts and does not close itself. Every [`CodeOffer::challenge`]
/// call lets one phone test one guess, so the caller counts each call as an attempt, limits them,
/// and drops the offer after the first confirmation that verifies.
pub struct CodeOffer {
    setup: ServerSetup<Suite>,
    registration: ServerRegistration<Suite>,
}

impl CodeOffer {
    /// Registers `code`. Its format and length are the caller's choice; an empty code is refused.
    #[must_use]
    pub fn new(code: &[u8]) -> Option<Self> {
        if code.is_empty() {
            return None;
        }
        let mut rng = OsRng;
        let setup = ServerSetup::new(&mut rng);
        let client = ClientRegistration::<Suite>::start(&mut rng, code).ok()?;
        let response = ServerRegistration::start(&setup, client.message, OFFER_ID).ok()?;
        let upload = client
            .state
            .finish(
                &mut rng,
                code,
                response.message,
                ClientRegistrationFinishParameters::default(),
            )
            .ok()?;
        Some(Self {
            setup,
            registration: ServerRegistration::finish(upload.message),
        })
    }

    #[must_use]
    pub fn challenge(
        &self,
        request: &[u8],
        binding: &[u8; BINDING_LEN],
        version: u64,
    ) -> Option<(CodeConfirmation, Vec<u8>)> {
        let context = context(binding, version);
        let result = ServerLogin::start(
            &mut OsRng,
            &self.setup,
            Some(self.registration.clone()),
            CredentialRequest::deserialize(request).ok()?,
            OFFER_ID,
            ServerLoginParameters {
                context: Some(&context),
                ..Default::default()
            },
        )
        .ok()?;
        Some((
            CodeConfirmation(result.state),
            result.message.serialize().to_vec(),
        ))
    }
}

/// The computer's side of one login attempt, made by [`CodeOffer::challenge`].
pub struct CodeConfirmation(ServerLogin<Suite>);

impl CodeConfirmation {
    /// Whether `message` is the confirmation of the phone that answered this challenge. The TLS
    /// binding and the version are tied in by the challenge, so a confirmation made for another
    /// challenge, connection or version does not verify.
    #[must_use]
    pub fn verify(self, message: &[u8]) -> bool {
        let Ok(message) = CredentialFinalization::deserialize(message) else {
            return false;
        };
        self.0
            .finish(message, ServerLoginParameters::default())
            .is_ok()
    }
}

/// A decoded OPAQUE challenge suitable for proof verification.
pub struct CodeChallenge(CredentialResponse<Suite>);

impl CodeChallenge {
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        // The deserializer consumes the fields but permits trailing bytes.
        if bytes.len() != CredentialResponseLen::<Suite>::USIZE {
            return None;
        }
        CredentialResponse::deserialize(bytes).ok().map(Self)
    }
}

pub struct CodeRequest(ClientLogin<Suite>);

impl CodeRequest {
    #[must_use]
    pub fn new(code: &[u8]) -> Option<(Self, Vec<u8>)> {
        if code.is_empty() {
            return None;
        }
        let result = ClientLogin::<Suite>::start(&mut OsRng, code).ok()?;
        Some((Self(result.state), result.message.serialize().to_vec()))
    }

    /// Produces the phone's confirmation only after authenticating the computer.
    #[must_use]
    pub fn confirm(
        self,
        code: &[u8],
        response: CodeChallenge,
        binding: &[u8; BINDING_LEN],
        version: u64,
    ) -> Option<Vec<u8>> {
        let context = context(binding, version);
        let result = self
            .0
            .finish(
                &mut OsRng,
                code,
                response.0,
                ClientLoginFinishParameters::new(Some(&context), Identifiers::default(), None),
            )
            .ok()?;
        Some(result.message.serialize().to_vec())
    }
}

#[derive(Clone, Copy)]
pub enum Role {
    Phone,
    Agent,
}

fn qr_message(role: Role, fingerprint: &[u8; 32]) -> Vec<u8> {
    let role: &[u8] = match role {
        Role::Phone => b"phone",
        Role::Agent => b"agent",
    };
    [b"construct-pair\0".as_slice(), role, fingerprint].concat()
}

/// The proof a phone or a computer makes from the QR secret: an HMAC-SHA-256 keyed with the
/// 32-byte secret over `construct-pair`, a zero byte, the role (`phone` or `agent`) and the
/// SHA-256 fingerprint of the computer's certificate.
#[must_use]
pub fn qr_proof(secret: &[u8; 32], role: Role, fingerprint: &[u8; 32]) -> hmac::Tag {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    hmac::sign(&key, &qr_message(role, fingerprint))
}

#[must_use]
pub fn verify_qr(secret: &[u8; 32], role: Role, fingerprint: &[u8; 32], proof: &[u8]) -> bool {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    hmac::verify(&key, &qr_message(role, fingerprint), proof).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_opaque_challenge_has_one_exact_length_and_valid_group_elements() {
        let offer = CodeOffer::new(b"01234567").unwrap();
        let (_, request) = CodeRequest::new(b"01234567").unwrap();
        let (_, mut challenge) = offer.challenge(&request, &[1; 32], 1).unwrap();
        assert!(CodeChallenge::parse(&challenge).is_some());
        assert!(CodeChallenge::parse(&challenge[..challenge.len() - 1]).is_none());
        assert!(CodeChallenge::parse(&vec![0; challenge.len()]).is_none());
        challenge.push(0);
        assert!(CodeChallenge::parse(&challenge).is_none());
    }

    #[test]
    fn a_qr_proof_binds_the_secret_the_role_and_the_certificate() {
        let proof = qr_proof(&[7; 32], Role::Phone, &[9; 32]);
        assert!(verify_qr(&[7; 32], Role::Phone, &[9; 32], proof.as_ref()));
        assert!(!verify_qr(&[8; 32], Role::Phone, &[9; 32], proof.as_ref()));
        assert!(!verify_qr(&[7; 32], Role::Agent, &[9; 32], proof.as_ref()));
        assert!(!verify_qr(&[7; 32], Role::Phone, &[10; 32], proof.as_ref()));
        assert!(!verify_qr(
            &[7; 32],
            Role::Phone,
            &[9; 32],
            &proof.as_ref()[1..]
        ));
    }

    #[test]
    fn the_qr_proofs_match_values_made_apart_from_this_code() {
        // HMAC-SHA-256 made with Python's hmac and hashlib over "construct-pair", a zero byte,
        // the role and the fingerprint, keyed with the secret. The same values pin the format
        // that phones and computers already in use speak.
        let fingerprint: [u8; 32] = std::array::from_fn(|i| u8::try_from(i).unwrap());
        let secret: [u8; 32] = std::array::from_fn(|i| 100 + u8::try_from(i).unwrap());
        let bytes = |hex: &str| -> Vec<u8> {
            (0..hex.len())
                .step_by(2)
                .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
                .collect()
        };
        assert_eq!(
            qr_proof(&secret, Role::Phone, &fingerprint).as_ref(),
            bytes("24d3b6880809f7fa38564b20d0442b7ae18ca931a8537eaccea4becc7ed751b5")
        );
        assert_eq!(
            qr_proof(&secret, Role::Agent, &fingerprint).as_ref(),
            bytes("3645785c711cf914b4148588aa38471121bb4e332d8de0bbd21c9940e56aaaf5")
        );
    }

    /// One login attempt on `offer`: the computer's state and the phone's valid confirmation.
    fn attempt(offer: &CodeOffer) -> (CodeConfirmation, Vec<u8>) {
        let (client, request) = CodeRequest::new(b"01234567").unwrap();
        let (computer, challenge) = offer.challenge(&request, &[1; 32], 1).unwrap();
        let response = CodeChallenge::parse(&challenge).unwrap();
        let confirmation = client.confirm(b"01234567", response, &[1; 32], 1).unwrap();
        (computer, confirmation)
    }

    #[test]
    fn the_computer_refuses_a_confirmation_that_is_not_valid_for_its_session() {
        let offer = CodeOffer::new(b"01234567").unwrap();
        let (computer, confirmation) = attempt(&offer);
        assert!(computer.verify(&confirmation));

        // The same offer, another attempt: a valid confirmation of the first does not pass.
        let (other, _) = attempt(&offer);
        assert!(!other.verify(&confirmation));

        let mut flipped = confirmation.clone();
        flipped[0] ^= 1;
        for wrong in [Vec::new(), vec![0; confirmation.len()], flipped] {
            let (computer, _) = attempt(&offer);
            assert!(!computer.verify(&wrong));
        }
    }

    #[test]
    fn an_empty_code_is_refused_on_both_sides() {
        assert!(CodeOffer::new(b"").is_none());
        assert!(CodeRequest::new(b"").is_none());
    }

    #[test]
    fn typed_code_checks_both_peers_and_the_tls_channel() {
        let offer = CodeOffer::new(b"01234567").unwrap();
        for (code, client_binding, version) in [
            (b"01234567", [1; 32], 1),
            (b"01234568", [1; 32], 1),
            (b"01234567", [2; 32], 1),
            (b"01234567", [1; 32], 0),
        ] {
            let (client, request) = CodeRequest::new(code).unwrap();
            let (computer, challenge) = offer.challenge(&request, &[1; 32], 1).unwrap();
            let response = CodeChallenge::parse(&challenge).unwrap();
            let confirmation = client.confirm(code, response, &client_binding, version);
            if code == b"01234567" && client_binding == [1; 32] && version == 1 {
                assert!(computer.verify(&confirmation.unwrap()));
            } else {
                assert!(confirmation.is_none());
            }
        }
    }
}
