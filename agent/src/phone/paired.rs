//! The phones that have paired: one file, `phones.json`, in the config directory.
//!
//! The file keeps a hash of each phone's token, never the token itself, so a copy of the file
//! cannot be used to talk to the agent. It is user-only and replaced in one step by
//! `host::write_private`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::Error;
use crate::host::write_private;

/// File name inside the config directory.
pub const FILE_NAME: &str = "phones.json";
const VERSION: u32 = 1;
/// The longest phone name that is kept, in characters.
const MAX_NAME: usize = 64;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Phone {
    name: String,
    token_hash: String,
    paired_at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
struct Content {
    version: u32,
    phones: Vec<Phone>,
}

impl Default for Content {
    fn default() -> Self {
        Self {
            version: VERSION,
            phones: Vec::new(),
        }
    }
}

/// The paired phones, in memory and in the file.
pub struct Paired {
    path: PathBuf,
    phones: Mutex<Vec<Phone>>,
}

fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// The name a phone gave itself, or a plain one when it is empty or has a control character.
fn clean_name(name: &str) -> String {
    let name: String = name.trim().chars().take(MAX_NAME).collect();
    if name.is_empty() || name.chars().any(char::is_control) {
        "Phone".to_owned()
    } else {
        name
    }
}

impl Paired {
    /// Read the file in `dir`; a missing file is an empty list.
    ///
    /// # Errors
    /// Fails when the file exists and cannot be read or understood: the agent does not start
    /// with an empty list over a file it could not read, nor over one of a newer format.
    pub fn load(dir: &Path) -> Result<Self, Error> {
        let path = dir.join(FILE_NAME);
        let phones = match fs::read(&path) {
            Ok(bytes) => {
                let content =
                    serde_json::from_slice::<Content>(&bytes).map_err(|_| Error::Storage)?;
                // A file of a newer format is not read and rewritten as this one: fields would be lost.
                if content.version > VERSION {
                    return Err(Error::Storage);
                }
                content.phones
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(_) => return Err(Error::Storage),
        };
        Ok(Self {
            path,
            phones: Mutex::new(phones),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Phone>> {
        self.phones.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Add a phone and give back its token, which is shown to the phone once.
    ///
    /// # Errors
    /// Fails when the token cannot be made or the file cannot be written.
    pub fn add(&self, name: &str) -> Result<String, Error> {
        let token = crate::host::generate_token().map_err(|_| Error::Random)?;
        let phone = Phone {
            name: clean_name(name),
            token_hash: hash_token(&token),
            paired_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        };
        let mut phones = self.lock();
        let mut next = phones.clone();
        next.push(phone);
        let content = Content {
            version: VERSION,
            phones: next,
        };
        let data = serde_json::to_vec(&content).map_err(|_| Error::Storage)?;
        write_private(&self.path, &data).map_err(|_| Error::Storage)?;
        *phones = content.phones;
        Ok(token)
    }

    /// Whether `token` belongs to a paired phone.
    #[must_use]
    pub fn contains(&self, token: &str) -> bool {
        let hash = hash_token(token);
        self.lock().iter().any(|p| p.token_hash == hash)
    }

    /// How many phones are paired.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
