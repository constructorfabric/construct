//! Reads one object store of a Chromium `IndexedDB` from its files on disk: the `LevelDB` directory
//! (`<origin>.indexeddb.leveldb`) and the directory of the values kept outside it
//! (`<origin>.indexeddb.blob`). Nothing is written and the `LevelDB` lock is never taken, so a copy
//! of a database that a browser holds open is read as well as a closed one.
//!
//! The key coding follows Chromium's `indexed_db_leveldb_coding.cc`, and the value wrapping its
//! `idb_value_wrapping.cc`: a value may be compressed with Snappy, or kept in a file of the blob
//! directory, before it is the serialized value that [`crate::v8`] reads.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::leveldb::{self, varint};
use crate::v8;

/// The prefix of the keys Chromium keeps its transaction scopes under: the global metadata
/// prefix, then the scopes byte.
const SCOPES_PREFIX: [u8; 5] = [0, 0, 0, 0, 50];
/// The scope key type of the metadata of one scope.
const SCOPE_METADATA: u8 = 1;
/// The scope key type of the log of one scope, which holds its undo tasks and its cleanup tasks.
const SCOPE_LOG: u8 = 2;
/// The log entries of a scope that are undo tasks.
const UNDO_TASK: u8 = 0;

/// The type byte of a key that maps a database name to its id.
const DATABASE_NAME_TYPE: u8 = 201;
/// The type byte of a key that holds metadata of an object store.
const OBJECT_STORE_META_TYPE: u8 = 50;
/// The metadata of an object store that is its name.
const OBJECT_STORE_NAME: u8 = 0;
/// The index id of the metadata of a database and of the global metadata.
const METADATA_INDEX: u8 = 0;
/// The index id of the records of an object store.
const RECORDS_INDEX: u8 = 1;
/// The index id of the list of files that hold values of an object store.
const BLOBS_INDEX: u8 = 3;
/// The type byte of a string key.
const STRING_KEY: u8 = 1;
/// The version a serialized value has when a wrapper stands in front of it.
const WRAPPED_VERSION: u8 = 17;
/// A wrapper that says the value is in a blob file.
const IN_BLOB_FILE: u8 = 1;
/// A wrapper that says the value is compressed with Snappy.
const SNAPPY: u8 = 2;
/// The tag of the offset of the trailer Blink writes from version 21 on.
const TRAILER_OFFSET: u8 = 0xFE;
/// How many wrappers may stand in front of one value.
const MAX_WRAPPERS: usize = 4;
/// The largest value that is read from a blob file or uncompressed, as leveldb-core bounds a
/// block: the length is taken from the file or the Snappy header before anything is allocated.
const MAX_VALUE_BYTES: u64 = 256 << 20;

/// Why an object store could not be read.
#[derive(Debug)]
pub enum Error {
    /// The `LevelDB` directory, or one of its record files, cannot be read.
    Unreadable(PathBuf, String),
    /// A copy of the files cannot be made at this path.
    Uncopyable(PathBuf, String),
    /// The files hold no database of this name.
    NoDatabase(String),
    /// The database holds no object store of this name.
    NoStore(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(path, reason) => write!(f, "cannot read {}: {reason}", path.display()),
            Self::Uncopyable(path, reason) => {
                write!(f, "cannot copy to {}: {reason}", path.display())
            }
            Self::NoDatabase(name) => write!(f, "no database {name}"),
            Self::NoStore(name) => write!(f, "no object store {name}"),
        }
    }
}

impl std::error::Error for Error {}

/// Why the value of one record could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueError {
    /// The value is kept in a blob file that cannot be read, for example one the browser
    /// removed after it wrote a newer value. Another read may find the newer one.
    BlobFile(PathBuf, String),
    /// The bytes are not a value this reader knows how to read.
    Format(String),
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlobFile(path, reason) => write!(f, "cannot read {}: {reason}", path.display()),
            Self::Format(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for ValueError {}

/// One record of an object store: its key and its value, or why the value cannot be read.
#[derive(Debug)]
pub struct StoredValue {
    pub key: String,
    pub value: Result<Value, ValueError>,
}

/// The files of one origin's `IndexedDB`.
pub struct Files<'a> {
    pub leveldb: &'a Path,
    pub blobs: &'a Path,
}

/// The newest value of every record with a string key in the object store `store` of the
/// database `database`, sorted by key. A record whose newest version is a deletion is left out.
///
/// # Errors
/// Fails when the `LevelDB` directory or one of its record files cannot be read, and when the
/// database or the store is not there. A value that cannot be read is an `Err` on its own
/// record, so one bad record does not hide the others.
pub fn read_store(
    files: &Files<'_>,
    database: &str,
    store: &str,
) -> Result<Vec<StoredValue>, Error> {
    let records = leveldb::read_live_records(files.leveldb)?;
    let mut live = newest_versions(records);
    revert_unfinished_transactions(&mut live)
        .map_err(|reason| Error::Unreadable(files.leveldb.to_owned(), reason))?;
    let database_id =
        database_id(&live, database).ok_or_else(|| Error::NoDatabase(database.to_owned()))?;
    let store_id =
        store_id(&live, database_id, store).ok_or_else(|| Error::NoStore(store.to_owned()))?;
    let records_prefix = key_prefix(database_id, store_id, RECORDS_INDEX);
    let blobs_prefix = key_prefix(database_id, store_id, BLOBS_INDEX);
    let mut values = Vec::new();
    for (key, value) in with_prefix(&live, &records_prefix) {
        let tail = &key[records_prefix.len()..];
        let Some(name) = string_key(tail) else {
            continue;
        };
        let blob_list = live
            .get([blobs_prefix.as_slice(), tail].concat().as_slice())
            .map(Vec::as_slice);
        let value = record_value(value, blob_list, files.blobs, database_id);
        values.push(StoredValue { key: name, value });
    }
    values.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(values)
}

/// The value of every key at its newest version; a key whose newest version is a deletion is
/// left out.
fn newest_versions(records: Vec<leveldb_core::Record>) -> Live {
    let mut newest: BTreeMap<Vec<u8>, leveldb_core::Record> = BTreeMap::new();
    for record in records {
        match newest.get(&record.key) {
            Some(known) if known.seq >= record.seq => {}
            _ => {
                newest.insert(record.key.clone(), record);
            }
        }
    }
    newest
        .into_iter()
        .filter(|(_, record)| !record.deleted)
        .map(|(key, record)| (key, record.value))
        .collect()
}

/// The keys and values of the database, in key order.
type Live = BTreeMap<Vec<u8>, Vec<u8>>;

fn with_prefix<'a>(
    live: &'a Live,
    prefix: &'a [u8],
) -> impl Iterator<Item = (&'a Vec<u8>, &'a Vec<u8>)> {
    live.range(prefix.to_vec()..)
        .take_while(move |(key, _)| key.starts_with(prefix))
}

/// Puts back what the transactions that were not committed changed, as Chromium does when it
/// opens the database. A large transaction is written in several batches, each with the steps
/// that undo it; its scope is committed when the locks in its metadata are cleared, and an
/// uncommitted scope is undone, newest step first.
fn revert_unfinished_transactions(live: &mut Live) -> Result<(), String> {
    let metadata = [SCOPES_PREFIX.as_slice(), &[SCOPE_METADATA]].concat();
    let mut unfinished = Vec::new();
    for (key, value) in with_prefix(live, &metadata) {
        if has_locks(value).ok_or("the metadata of a transaction cannot be read")? {
            unfinished.push(key[metadata.len()..].to_vec());
        }
    }
    for scope in unfinished {
        let undo = [SCOPES_PREFIX.as_slice(), &[SCOPE_LOG], &scope, &[UNDO_TASK]].concat();
        let steps: Vec<Vec<u8>> = with_prefix(live, &undo).map(|(_, v)| v.clone()).collect();
        for step in steps {
            undo_step(live, &step).ok_or("an undo step of a transaction cannot be read")?;
        }
    }
    Ok(())
}

/// Whether the metadata of a scope lists locks (field 1): the scope is not committed. `None` for
/// metadata that cannot be read.
fn has_locks(metadata: &[u8]) -> Option<bool> {
    proto_fields(metadata).map(|fields| fields.iter().any(|(field, _)| *field == 1))
}

/// Applies one step of an undo log: put a key back (field 1), delete a key (field 2), or
/// delete a range of keys (field 3).
fn undo_step(live: &mut Live, step: &[u8]) -> Option<()> {
    let (operation, body) = *proto_fields(step)?.first()?;
    let fields = proto_fields(body)?;
    let field = |n: u64| {
        fields
            .iter()
            .find(|(f, _)| *f == n)
            .map(|(_, v)| v.to_vec())
    };
    match operation {
        1 => {
            live.insert(field(1)?, field(2).unwrap_or_default());
        }
        2 => {
            live.remove(&field(1)?);
        }
        // Chromium reverts a range without its end: the undo step has no flag for it.
        3 => {
            let (begin, end) = (field(1)?, field(2)?);
            if begin > end {
                return None;
            }
            let doomed: Vec<Vec<u8>> = live.range(begin..end).map(|(k, _)| k.clone()).collect();
            for key in doomed {
                live.remove(&key);
            }
        }
        _ => return None,
    }
    Some(())
}

/// The fields of a protobuf message with their bytes; a number field gives no bytes. `None` for
/// bytes that are not such a message.
fn proto_fields(bytes: &[u8]) -> Option<Vec<(u64, &[u8])>> {
    let mut fields = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        let (tag, after) = varint(rest)?;
        let (field, wire_type) = (tag >> 3, tag & 7);
        rest = match wire_type {
            0 => varint(after)?.1,
            1 => after.get(8..)?,
            2 => {
                let (len, after) = varint(after)?;
                let len = usize::try_from(len).ok()?;
                fields.push((field, after.get(..len)?));
                after.get(len..)?
            }
            5 => after.get(4..)?,
            _ => return None,
        };
        if wire_type != 2 {
            fields.push((field, &[]));
        }
    }
    Some(fields)
}

/// The key prefix of a database, object store and index: one byte with the byte length of each
/// id less one, then the ids, little-endian. The index ids used here take one byte.
fn key_prefix(database: u64, store: u64, index: u8) -> Vec<u8> {
    let (database, store) = (le_bytes(database), le_bytes(store));
    // Each length is 1 to 8, so the head fits in a byte.
    #[allow(clippy::cast_possible_truncation)]
    let head = (((database.len() - 1) << 5) | ((store.len() - 1) << 2)) as u8;
    let mut prefix = vec![head];
    prefix.extend(database);
    prefix.extend(store);
    prefix.push(index);
    prefix
}

/// The shortest little-endian bytes of a number, one at least.
fn le_bytes(value: u64) -> Vec<u8> {
    let bytes = value.to_le_bytes();
    let len = bytes.iter().rposition(|b| *b != 0).map_or(1, |i| i + 1);
    bytes[..len].to_vec()
}

fn le_number(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() || bytes.len() > 8 {
        return None;
    }
    Some(
        bytes
            .iter()
            .rev()
            .fold(0_u64, |acc, byte| (acc << 8) | u64::from(*byte)),
    )
}

/// A string written as its length in characters and its UTF-16 big-endian characters.
fn string_with_length(bytes: &[u8]) -> Option<(String, &[u8])> {
    let (len, rest) = varint(bytes)?;
    let len = usize::try_from(len).ok()?.checked_mul(2)?;
    let text = rest.get(..len)?;
    Some((utf16_be(text), &rest[len..]))
}

fn utf16_be(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_be_bytes(*pair))
        .collect();
    String::from_utf16_lossy(&units)
}

/// The id of the database named `name`, from the key that maps its origin and name to it.
fn database_id(live: &Live, name: &str) -> Option<u64> {
    let mut global = key_prefix(0, 0, METADATA_INDEX);
    global.push(DATABASE_NAME_TYPE);
    with_prefix(live, &global).find_map(|(key, value)| {
        let rest = key.strip_prefix(global.as_slice())?;
        let (_origin, rest) = string_with_length(rest)?;
        let (database, rest) = string_with_length(rest)?;
        (rest.is_empty() && database == name).then(|| le_number(value))?
    })
}

/// The id of the object store named `name` in the database.
fn store_id(live: &Live, database: u64, name: &str) -> Option<u64> {
    let mut meta = key_prefix(database, 0, METADATA_INDEX);
    meta.push(OBJECT_STORE_META_TYPE);
    with_prefix(live, &meta).find_map(|(key, value)| {
        let rest = key.strip_prefix(meta.as_slice())?;
        let (store, rest) = varint(rest)?;
        (rest == [OBJECT_STORE_NAME] && utf16_be(value) == name).then_some(store)
    })
}

/// A string key; `None` for a key of another type.
fn string_key(bytes: &[u8]) -> Option<String> {
    let rest = bytes.strip_prefix(&[STRING_KEY])?;
    let (key, rest) = string_with_length(rest)?;
    rest.is_empty().then_some(key)
}

/// The value of a record: its version, then the wrapped or serialized value.
fn record_value(
    value: &[u8],
    blob_list: Option<&[u8]>,
    blobs: &Path,
    database: u64,
) -> Result<Value, ValueError> {
    let invalid = |reason: &str| ValueError::Format(reason.to_owned());
    let (_version, rest) = varint(value).ok_or_else(|| invalid("record without a version"))?;
    let mut bytes = rest.to_vec();
    for _ in 0..=MAX_WRAPPERS {
        match bytes.as_slice() {
            [0xFF, WRAPPED_VERSION, IN_BLOB_FILE, rest @ ..] => {
                let (_size, rest) =
                    varint(rest).ok_or_else(|| invalid("blob reference without a size"))?;
                let (index, _) =
                    varint(rest).ok_or_else(|| invalid("blob reference without an index"))?;
                let list = blob_list
                    .ok_or_else(|| invalid("value in a blob file, but no list of the files"))?;
                let number =
                    blob_number(list, index).ok_or_else(|| invalid("blob file not in the list"))?;
                let path = blob_path(blobs, database, number);
                let unreadable =
                    |e: std::io::Error| ValueError::BlobFile(path.clone(), e.to_string());
                if std::fs::metadata(&path).map_err(unreadable)?.len() > MAX_VALUE_BYTES {
                    return Err(invalid("value in a blob file too large"));
                }
                bytes = std::fs::read(&path).map_err(unreadable)?;
            }
            [0xFF, WRAPPED_VERSION, SNAPPY, rest @ ..] => {
                let len = snap::raw::decompress_len(rest)
                    .map_err(|e| invalid(&format!("cannot uncompress the value: {e}")))?;
                if len as u64 > MAX_VALUE_BYTES {
                    return Err(invalid("compressed value too large"));
                }
                bytes = snap::raw::Decoder::new()
                    .decompress_vec(rest)
                    .map_err(|e| invalid(&format!("cannot uncompress the value: {e}")))?;
            }
            _ => return serialized_value(&bytes).map_err(|e| invalid(&e.to_string())),
        }
    }
    Err(invalid("too many wrappers"))
}

/// A value as Blink serializes it: its version, the trailer offset from version 21 on, then the
/// V8 value.
fn serialized_value(bytes: &[u8]) -> Result<Value, v8::Error> {
    let not_blink = || v8::Error {
        reason: "no Blink version header".into(),
        offset: 0,
    };
    let rest = bytes.strip_prefix(&[0xFF]).ok_or_else(not_blink)?;
    let (_version, rest) = varint(rest).ok_or_else(not_blink)?;
    let rest = match rest.strip_prefix(&[TRAILER_OFFSET]) {
        Some(after) => after.get(12..).ok_or_else(not_blink)?,
        None => rest,
    };
    v8::read(rest)
}

/// The number of the `index`-th file in a record's list of blob files. Each entry is a flag for
/// a file with a name, the number, the type, then the size or the name and the change time.
fn blob_number(list: &[u8], index: u64) -> Option<u64> {
    let mut rest = list;
    let mut i = 0;
    loop {
        let (&named, after) = rest.split_first()?;
        let (number, after) = varint(after)?;
        let (_type, after) = string_with_length(after)?;
        rest = if named == 0 {
            varint(after)?.1
        } else {
            let (_name, after) = string_with_length(after)?;
            varint(after)?.1
        };
        if i == index {
            return Some(number);
        }
        i += 1;
    }
}

/// Where blob file `number` of `database` is kept: a folder named by the database id in hex,
/// then one by the second byte of the number, then the number in hex.
fn blob_path(blobs: &Path, database: u64, number: u64) -> PathBuf {
    blobs
        .join(format!("{database:x}"))
        .join(format!("{:02x}", (number >> 8) & 0xFF))
        .join(format!("{number:x}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_prefix_holds_the_byte_length_of_each_id() {
        assert_eq!(key_prefix(1, 1, 1), [0x00, 1, 1, 1]);
        assert_eq!(key_prefix(0, 0, 0), [0x00, 0, 0, 0]);
        assert_eq!(key_prefix(300, 2, 3), [0x20, 0x2C, 0x01, 2, 3]);
        assert_eq!(key_prefix(1, 70_000, 1), [0x08, 1, 0x70, 0x11, 0x01, 1]);
    }

    #[test]
    fn a_blob_file_is_found_by_its_place_in_the_list() {
        let kind: Vec<u8> = "t".encode_utf16().flat_map(u16::to_be_bytes).collect();
        let mut list = vec![0, 2, 1];
        list.extend(&kind);
        list.push(5);
        list.extend([1, 0x81, 0x02, 1]);
        list.extend(&kind);
        list.push(1);
        list.extend(&kind);
        list.push(9);
        assert_eq!(blob_number(&list, 0), Some(2));
        assert_eq!(blob_number(&list, 1), Some(257));
        assert_eq!(blob_number(&list, 2), None);
        assert_eq!(blob_path(Path::new("/b"), 1, 257), Path::new("/b/1/01/101"));
        // The folder of a database is its id in hex.
        assert_eq!(blob_path(Path::new("/b"), 11, 2), Path::new("/b/b/00/2"));
    }

    /// A protobuf message of byte fields.
    fn message(fields: &[(u8, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (field, bytes) in fields {
            out.push((field << 3) | 2);
            out.push(u8::try_from(bytes.len()).unwrap());
            out.extend_from_slice(bytes);
        }
        out
    }

    fn scope_key(kind: u8, scope: u8, tail: &[u8]) -> Vec<u8> {
        [SCOPES_PREFIX.as_slice(), &[kind, scope], tail].concat()
    }

    /// An undo step of `scope`: `step` counts down from the first step, as Chromium writes.
    fn undo(scope: u8, step: u64, operation: u8, body: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let order = (i64::MAX as u64 - step).to_be_bytes();
        let key = scope_key(SCOPE_LOG, scope, &[[UNDO_TASK].as_slice(), &order].concat());
        (key, message(&[(operation, body)]))
    }

    #[test]
    fn an_unfinished_transaction_is_undone_newest_step_first_and_a_committed_one_is_kept() {
        let mut live: Live = BTreeMap::new();
        // Scope 7 is not committed: its metadata lists a lock. It set `k` twice, wrote `new`,
        // and deleted `a` and `b`.
        live.insert(scope_key(SCOPE_METADATA, 7, &[]), message(&[(1, b"")]));
        live.insert(b"k".to_vec(), b"second".to_vec());
        live.insert(b"new".to_vec(), b"x".to_vec());
        for step in [
            undo(7, 0, 1, &message(&[(1, b"k"), (2, b"before")])),
            undo(7, 1, 1, &message(&[(1, b"k"), (2, b"first")])),
            undo(7, 2, 2, &message(&[(1, b"new")])),
            undo(7, 3, 1, &message(&[(1, b"a"), (2, b"A")])),
            undo(7, 4, 1, &message(&[(1, b"b"), (2, b"B")])),
        ] {
            live.insert(step.0, step.1);
        }
        // A cleanup task in the same log (entry type 1, not an undo task): not applied.
        let cleanup = scope_key(SCOPE_LOG, 7, &[1, 0, 0, 0, 0, 0, 0, 0, 1]);
        live.insert(cleanup, message(&[(2, &message(&[(1, b"a"), (2, b"b")]))]));
        // Scope 8 is committed (no locks) but not cleaned up: its undo log is not applied.
        live.insert(scope_key(SCOPE_METADATA, 8, &[]), Vec::new());
        live.insert(b"kept".to_vec(), b"after".to_vec());
        let (key, value) = undo(8, 0, 2, &message(&[(1, b"kept")]));
        live.insert(key, value);
        // A range deletion of an unfinished scope.
        live.insert(scope_key(SCOPE_METADATA, 9, &[]), message(&[(1, b"")]));
        live.insert(b"r1".to_vec(), Vec::new());
        live.insert(b"r2".to_vec(), Vec::new());
        let (key, value) = undo(9, 0, 3, &message(&[(1, b"r"), (2, b"s")]));
        live.insert(key, value);

        revert_unfinished_transactions(&mut live).unwrap();
        let data: Vec<(&[u8], &[u8])> = live
            .iter()
            .filter(|(key, _)| !key.starts_with(&SCOPES_PREFIX))
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
            .collect();
        assert_eq!(
            data,
            [
                (b"a".as_slice(), b"A".as_slice()),
                (b"b", b"B"),
                (b"k", b"before"),
                (b"kept", b"after"),
            ]
        );
    }

    #[test]
    fn unreadable_metadata_or_a_reversed_range_of_an_unfinished_transaction_is_an_error() {
        let mut damaged: Live = BTreeMap::new();
        damaged.insert(scope_key(SCOPE_METADATA, 7, &[]), vec![0xFF]);
        assert_eq!(
            revert_unfinished_transactions(&mut damaged),
            Err("the metadata of a transaction cannot be read".to_owned())
        );
        let mut reversed: Live = BTreeMap::new();
        reversed.insert(scope_key(SCOPE_METADATA, 9, &[]), message(&[(1, b"")]));
        let (key, value) = undo(9, 0, 3, &message(&[(1, b"s"), (2, b"r")]));
        reversed.insert(key, value);
        assert_eq!(
            revert_unfinished_transactions(&mut reversed),
            Err("an undo step of a transaction cannot be read".to_owned())
        );
    }

    #[test]
    fn a_compressed_value_that_claims_more_than_the_limit_is_refused_before_it_is_made() {
        // A Snappy header that says 300 MB, and no data.
        let record = [
            8,
            0xFF,
            WRAPPED_VERSION,
            SNAPPY,
            0x80,
            0x80,
            0x80,
            0x8F,
            0x01,
        ];
        assert_eq!(
            record_value(&record, None, Path::new("/b"), 1),
            Err(ValueError::Format("compressed value too large".to_owned()))
        );
    }

    /// `inner` wrapped `times` times in Snappy, as the value of a record.
    fn compressed(inner: &[u8], times: usize) -> Vec<u8> {
        let mut bytes = inner.to_vec();
        for _ in 0..times {
            let mut wrapped = vec![0xFF, WRAPPED_VERSION, SNAPPY];
            wrapped.extend(snap::raw::Encoder::new().compress_vec(&bytes).unwrap());
            bytes = wrapped;
        }
        let mut record = vec![8];
        record.extend(bytes);
        record
    }

    #[test]
    fn a_value_with_too_many_wrappers_or_none_it_knows_is_refused() {
        let invalid = |reason: &str| Err(ValueError::Format(reason.to_owned()));
        // The serialized value of `true`, behind as many wrappers as are read, and one more.
        let value = [0xFF, 0x0F, 0xFF, 0x0F, b'T'];
        assert_eq!(
            record_value(&compressed(&value, MAX_WRAPPERS), None, Path::new("/b"), 1),
            Ok(Value::Bool(true))
        );
        assert_eq!(
            record_value(
                &compressed(&value, MAX_WRAPPERS + 1),
                None,
                Path::new("/b"),
                1
            ),
            invalid("too many wrappers")
        );
        let in_blob_without_list = [8, 0xFF, WRAPPED_VERSION, IN_BLOB_FILE, 5, 0];
        assert_eq!(
            record_value(&in_blob_without_list, None, Path::new("/b"), 1),
            invalid("value in a blob file, but no list of the files")
        );
        assert_eq!(
            record_value(&[8, 0x01], None, Path::new("/b"), 1),
            invalid("no Blink version header at byte 0")
        );
    }
}
