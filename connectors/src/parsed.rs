use crate::Record;

/// What one line of a session file is, before it is handed on as an `Option<Record>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Parsed {
    /// A line of no interest to the reader.
    Ignored,
    /// A line that is not valid JSON of the expected shape, counted by the caller.
    Skipped,
    Record(Record),
}
