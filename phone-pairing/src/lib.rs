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
/// TLS ALPN identifier for typed pairing with OPAQUE.
pub const TYPED_CAPABILITY: &[u8] = b"construct-pair-code/opaque-rfc9807-v1";
/// The typed pairing offer cannot be used. QR and token requests have their own behavior.
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
pub struct CodeOffer {
    setup: ServerSetup<Suite>,
    registration: ServerRegistration<Suite>,
}

impl CodeOffer {
    #[must_use]
    pub fn new(code: &[u8]) -> Option<Self> {
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
            CodeConfirmation {
                state: result.state,
                context,
            },
            result.message.serialize().to_vec(),
        ))
    }
}

pub struct CodeConfirmation {
    state: ServerLogin<Suite>,
    context: Vec<u8>,
}

impl CodeConfirmation {
    #[must_use]
    pub fn verify(self, message: &[u8]) -> bool {
        let Ok(message) = CredentialFinalization::deserialize(message) else {
            return false;
        };
        self.state
            .finish(
                message,
                ServerLoginParameters {
                    context: Some(&self.context),
                    ..Default::default()
                },
            )
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

/// The existing QR proof, keyed with the 32-byte QR secret and bound to the certificate.
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
