//! The phones that have paired: one file, `phones.json`, in the config directory.
//!
//! The file keeps a hash of each phone's token, never the token itself, so a copy of the file
//! cannot be used to talk to the agent. A file with a newer version or a field this agent does not
//! know is refused, not read as version 1 and rewritten without the field. It is user-only and
//! replaced in one step by `host::write_private`.
//!
//! The phones live in this file, next to the certificate, and not in `settings.json`: the settings
//! file is read and rewritten by the app's settings commands and holds the model key, while this
//! one holds what proves a phone, and only the pairing code writes it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

use super::Error;
use crate::host::write_private;

/// File name inside the config directory.
pub const FILE_NAME: &str = "phones.json";
const VERSION: u32 = 1;
/// The longest phone name that is kept, in characters.
const MAX_NAME: usize = 64;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Phone {
    /// Names the phone to the app, for Unpair. It is random and has nothing to do with the token.
    id: String,
    name: String,
    token_hash: String,
    paired_at: u64,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
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

/// What the app may know of a paired phone: never the token or its hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PhoneInfo {
    pub id: String,
    pub name: String,
    /// Seconds since the Unix epoch.
    pub paired_at: u64,
}

impl From<&Phone> for PhoneInfo {
    fn from(phone: &Phone) -> Self {
        Self {
            id: phone.id.clone(),
            name: phone.name.clone(),
            paired_at: phone.paired_at,
        }
    }
}

/// The paired phones, in memory and in the file.
pub struct Paired {
    path: PathBuf,
    phones: Mutex<Vec<Phone>>,
    /// The id of each phone that was unpaired, for the channels it has open.
    revoked: broadcast::Sender<String>,
}

fn new_id() -> Result<String, Error> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|_| Error::Random)?;
    Ok(hex::encode(bytes))
}

fn write_phones(path: &Path, phones: &[Phone]) -> Result<(), Error> {
    let content = serde_json::json!({"version": VERSION, "phones": phones});
    let data = serde_json::to_vec(&content).map_err(|_| Error::Storage)?;
    write_private(path, &data).map_err(|_| Error::Storage)
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
                if content.version != VERSION {
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
            revoked: broadcast::channel(16).0,
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
            id: new_id()?,
            name: clean_name(name),
            token_hash: hash_token(&token),
            paired_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        };
        let mut phones = self.lock();
        let mut next = phones.clone();
        next.push(phone);
        write_phones(&self.path, &next)?;
        *phones = next;
        Ok(token)
    }

    /// The paired phones, oldest first.
    #[must_use]
    pub fn list(&self) -> Vec<PhoneInfo> {
        self.lock().iter().map(PhoneInfo::from).collect()
    }

    /// A page in stable id order. Deleting a phone cannot change the cursor for the next page.
    pub(super) fn page(&self, after: Option<&str>, limit: usize) -> Vec<PhoneInfo> {
        let phones = self.lock();
        let mut matching: Vec<_> = phones
            .iter()
            .filter(|phone| after.is_none_or(|id| phone.id.as_str() > id))
            .collect();
        matching.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        matching
            .into_iter()
            .take(limit)
            .map(PhoneInfo::from)
            .collect()
    }

    /// The id of the phone that owns `token`.
    #[must_use]
    pub fn id_of(&self, token: &str) -> Option<String> {
        let hash = hash_token(token);
        self.lock()
            .iter()
            .find(|p| p.token_hash == hash)
            .map(|p| p.id.clone())
    }

    /// Whether the phone with this id is still paired.
    #[must_use]
    pub fn has_id(&self, id: &str) -> bool {
        self.lock().iter().any(|p| p.id == id)
    }

    /// Hear the id of every phone that is unpaired from now on.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.revoked.subscribe()
    }

    /// Unpair the phone with this id; `false` when there is none. Its channels are told at once.
    ///
    /// # Errors
    /// Fails when the file cannot be written; then the phone stays paired.
    pub fn remove(&self, id: &str) -> Result<bool, Error> {
        let mut phones = self.lock();
        let Some(at) = phones.iter().position(|p| p.id == id) else {
            return Ok(false);
        };
        let mut next = phones.clone();
        next.remove(at);
        write_phones(&self.path, &next)?;
        *phones = next;
        // No receiver is fine: the phone has no channel open.
        let _ = self.revoked.send(id.to_owned());
        Ok(true)
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
