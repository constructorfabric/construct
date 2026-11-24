//! The agent's certificate for the phone endpoint.
//!
//! One self-signed certificate (ECDSA P-256), made on the first start and kept in the config
//! directory, the key in a user-only file. The phone does not trust it by name or by an
//! authority: it trusts exactly the certificate whose SHA-256 fingerprint the pairing gave it.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, version};
use sha2::{Digest, Sha256};

use super::Error;
use crate::host::write_private;

const CERT_FILE: &str = "phone-cert.der";
const KEY_FILE: &str = "phone-key.der";

/// The SHA-256 fingerprint of a DER certificate.
#[must_use]
pub fn fingerprint_of(der: &[u8]) -> [u8; 32] {
    Sha256::digest(der).into()
}

/// The certificate and its key.
pub struct Identity {
    cert: Vec<u8>,
    key: Vec<u8>,
}

fn read_optional(file: &Path) -> Result<Option<Vec<u8>>, Error> {
    match fs::read(file) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(Error::Storage),
    }
}

impl Identity {
    /// Read the certificate and key in `dir`, or make and keep them when neither exists.
    ///
    /// # Errors
    /// Fails when only one of the two files is there, or a file cannot be read or written.
    pub fn load_or_create(dir: &Path) -> Result<Self, Error> {
        let cert = read_optional(&dir.join(CERT_FILE))?;
        let key = read_optional(&dir.join(KEY_FILE))?;
        match (cert, key) {
            (Some(cert), Some(key)) => Ok(Self { cert, key }),
            (None, None) => {
                let made = rcgen::generate_simple_self_signed(vec!["construct-agent".to_owned()])
                    .map_err(|_| Error::Tls)?;
                let identity = Self {
                    cert: made.cert.der().to_vec(),
                    key: made.signing_key.serialize_der(),
                };
                fs::create_dir_all(dir).map_err(|_| Error::Storage)?;
                write_private(&dir.join(KEY_FILE), &identity.key).map_err(|_| Error::Storage)?;
                write_private(&dir.join(CERT_FILE), &identity.cert).map_err(|_| Error::Storage)?;
                Ok(identity)
            }
            _ => Err(Error::Storage),
        }
    }

    /// The fingerprint the phone pins.
    #[must_use]
    pub fn fingerprint(&self) -> [u8; 32] {
        fingerprint_of(&self.cert)
    }

    /// TLS 1.3 only, no client certificate, this certificate.
    ///
    /// # Errors
    /// Fails when the key is not usable.
    pub fn server_config(&self) -> Result<Arc<ServerConfig>, Error> {
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.clone()));
        let mut config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&version::TLS13])
                .map_err(|_| Error::Tls)?
                .with_no_client_auth()
                .with_single_cert(vec![CertificateDer::from(self.cert.clone())], key)
                .map_err(|_| Error::Tls)?;
        config.alpn_protocols = vec![phone_pairing::TYPED_CAPABILITY.to_vec()];
        Ok(Arc::new(config))
    }
}
