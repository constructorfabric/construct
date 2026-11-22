//! What the chat needs of the host's store of records: the conversations it keeps, as sessions of
//! the source `chat`.

use std::io;

use connectors::Record;

/// One session of a source, as listed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    /// The real id, read from the file, not its file name.
    pub session: String,
    /// The time of the last record (UTC RFC 3339).
    pub last_time: String,
    /// The text of the first user turn; empty when there is none.
    pub first_user_text: String,
}

/// The host's store. The chat holds it behind one lock and does several calls in a row under it,
/// so a call that reads and a call that deletes cannot interleave.
pub trait Records: Send {
    /// The records of one session, oldest first; none for a session that is not there.
    ///
    /// # Errors
    /// When the session cannot be read.
    fn read_session(&self, source: &str, session: &str) -> io::Result<Vec<Record>>;

    /// The sessions of a source, the newest first.
    ///
    /// # Errors
    /// When the source cannot be read.
    fn list_sessions(&self, source: &str) -> io::Result<Vec<SessionInfo>>;

    /// Add one record to its session. `false` when the store already had it.
    ///
    /// # Errors
    /// When the record cannot be written.
    fn append(&mut self, record: &Record) -> io::Result<bool>;

    /// Make the records of a session durable.
    ///
    /// # Errors
    /// When they cannot be made durable.
    fn flush_session(&mut self, source: &str, session: &str) -> io::Result<()>;

    /// Remove one session.
    ///
    /// # Errors
    /// When it cannot be removed.
    fn delete_session(&mut self, source: &str, session: &str) -> io::Result<()>;

    /// Remove every session of a source.
    ///
    /// # Errors
    /// When they cannot be removed.
    fn delete_source(&mut self, source: &str) -> io::Result<()>;
}
