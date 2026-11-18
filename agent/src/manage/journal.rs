//! What the sender keeps on disk: the journal of the prompts that were sent, and the time of the
//! agent's first start.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::host::write_private;

const STATE_FILE: &str = "manage-state.json";
const JOURNAL_FILE: &str = "manage-sent.log";
const FILE_MODE: u32 = 0o600;

/// The prompts that were sent, one line each: `+` and the key before the call, `-` and the key
/// when the call is known not to have reached the model, and `!` and the key for a prompt typed
/// while collection was off, which is never sent.
pub(super) struct Journal {
    file: File,
    pub(super) sent: HashSet<u64>,
    pub(super) refused: HashSet<u64>,
}

impl Journal {
    pub(super) fn open(dir: &Path) -> io::Result<Self> {
        let path = dir.join(JOURNAL_FILE);
        let mut text = Vec::new();
        match File::open(&path) {
            Ok(mut f) => {
                f.read_to_end(&mut text)?;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let mut sent = HashSet::new();
        let mut refused = HashSet::new();
        // A line that is not a sign and 16 hex digits is a torn one: it counts for nothing.
        for line in String::from_utf8_lossy(&text).lines() {
            let (sign, key) = line.split_at_checked(1).unwrap_or_default();
            if key.len() != 16 {
                continue;
            }
            let Ok(key) = u64::from_str_radix(key, 16) else {
                continue;
            };
            match sign {
                "+" => {
                    sent.insert(key);
                }
                "-" => {
                    sent.remove(&key);
                }
                "!" => {
                    refused.insert(key);
                }
                _ => {}
            }
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(FILE_MODE)
            .open(&path)?;
        if !text.is_empty() && text.last() != Some(&b'\n') {
            file.write_all(b"\n")?;
        }
        Ok(Self {
            file,
            sent,
            refused,
        })
    }

    fn append(&mut self, sign: char, key: u64) -> io::Result<()> {
        self.file
            .write_all(format!("{sign}{key:016x}\n").as_bytes())?;
        self.file.sync_data()
    }

    /// Whether the prompt was sent, or was refused and never will be.
    pub(super) fn is_done(&self, key: u64) -> bool {
        self.sent.contains(&key) || self.refused.contains(&key)
    }

    /// Marks the prompt as sent, on disk, before the call; false when it was sent already.
    pub(super) fn begin(&mut self, key: u64) -> io::Result<bool> {
        if self.is_done(key) {
            return Ok(false);
        }
        self.append('+', key)?;
        self.sent.insert(key);
        Ok(true)
    }

    /// Marks the prompt as never to be sent, on disk.
    pub(super) fn refuse(&mut self, key: u64) -> io::Result<()> {
        if self.sent.contains(&key) || !self.refused.insert(key) {
            return Ok(());
        }
        self.append('!', key)
    }

    /// Forgets every prompt: the file is emptied and so are the sets.
    pub(super) fn erase(&mut self) -> io::Result<()> {
        self.file.set_len(0)?;
        self.file.sync_all()?;
        self.sent.clear();
        self.refused.clear();
        Ok(())
    }

    /// Takes the mark back: the call did not reach the model.
    pub(super) fn undo(&mut self, key: u64) -> io::Result<()> {
        if self.sent.remove(&key) {
            self.append('-', key)?;
        }
        Ok(())
    }
}

/// The time of the agent's first start, kept in `dir`: what is older is history.
pub(super) fn first_start(dir: &Path) -> io::Result<String> {
    let path = dir.join(STATE_FILE);
    if let Ok(bytes) = fs::read(&path)
        && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
        && let Some(time) = value["first_start"].as_str()
        && OffsetDateTime::parse(time, &Rfc3339).is_ok()
    {
        return Ok(time.to_owned());
    }
    let now = days_ago(0)?;
    write_private(&path, json!({ "first_start": now }).to_string().as_bytes())?;
    Ok(now)
}

/// Now, minus `days`, as UTC RFC 3339.
///
/// # Errors
/// Fails when that time is out of the range of dates.
pub(super) fn days_ago(days: u32) -> io::Result<String> {
    OffsetDateTime::now_utc()
        .checked_sub(time::Duration::days(i64::from(days)))
        .ok_or_else(|| io::Error::other("that many days is out of the range of dates"))?
        .format(&Rfc3339)
        .map_err(io::Error::other)
}

#[cfg(test)]
// A failed `unwrap` in a test is a failed test.
#[allow(clippy::unwrap_used)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cb-manage-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_journal_remembers_across_opens_and_ignores_a_torn_line() {
        let dir = scratch("journal");
        let mut journal = Journal::open(&dir).unwrap();
        assert!(journal.begin(1).unwrap());
        assert!(!journal.begin(1).unwrap());
        assert!(journal.begin(2).unwrap());
        journal.undo(2).unwrap();
        drop(journal);
        // A crash in the middle of a line.
        let mut file = OpenOptions::new()
            .append(true)
            .open(dir.join(JOURNAL_FILE))
            .unwrap();
        file.write_all(b"+00000000000000").unwrap();
        drop(file);
        let mut journal = Journal::open(&dir).unwrap();
        assert!(!journal.begin(1).unwrap(), "sent before the restart");
        assert!(journal.begin(2).unwrap(), "taken back before the restart");
        assert!(journal.begin(3).unwrap());
        drop(journal);
        let again = Journal::open(&dir).unwrap();
        assert_eq!(again.sent, HashSet::from([1, 2, 3]));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_first_start_is_noted_once() {
        let dir = scratch("first");
        let first = first_start(&dir).unwrap();
        assert_eq!(first_start(&dir).unwrap(), first);
        fs::write(dir.join(STATE_FILE), "damaged").unwrap();
        assert!(OffsetDateTime::parse(&first_start(&dir).unwrap(), &Rfc3339).is_ok());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn an_absurd_number_of_days_is_an_error_not_a_panic() {
        assert!(days_ago(u32::MAX).is_err());
        assert!(days_ago(30).is_ok());
    }

    #[test]
    fn erasing_empties_the_file_and_the_sets_so_the_same_keys_count_again() {
        let dir = std::env::temp_dir().join(format!("cb-journal-erase-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut journal = Journal::open(&dir).unwrap();
        assert!(journal.begin(1).unwrap());
        journal.refuse(2).unwrap();
        assert!(journal.is_done(1) && journal.is_done(2));

        journal.erase().unwrap();

        assert!(journal.sent.is_empty() && journal.refused.is_empty());
        assert!(!journal.is_done(1) && !journal.is_done(2));
        assert_eq!(fs::metadata(dir.join(JOURNAL_FILE)).unwrap().len(), 0);
        // Appends go on from the start of the emptied file, and a reopen reads only them.
        assert!(journal.begin(1).unwrap());
        let reopened = Journal::open(&dir).unwrap();
        assert!(reopened.sent.contains(&1) && reopened.refused.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
