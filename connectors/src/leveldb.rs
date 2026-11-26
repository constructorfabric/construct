//! The files of a `LevelDB` directory that hold its current state, found the way `LevelDB` finds
//! them: `CURRENT` names the manifest, and the version edits of the manifest name the live table
//! files and the oldest log that is still live. A table that is still being written, or one that
//! a compaction left behind, is not live and is not read; so a copy taken while the database is
//! written reads as one state of it.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::indexeddb::Error;

/// The size of a block of a `LevelDB` log file.
const BLOCK_SIZE: usize = 32 * 1024;
/// The checksum, length and type in front of every piece of a log record.
const HEADER_SIZE: usize = 7;
/// The types of the pieces of a log record.
const FULL: u8 = 1;
const FIRST: u8 = 2;
const MIDDLE: u8 = 3;
const LAST: u8 = 4;
/// What `LevelDB` adds to a checksum it writes, so a checksum of checksums is not one.
const CRC_MASK_DELTA: u32 = 0xa282_ead8;
/// The kinds of the entries of a batch of writes.
const WRITE_DELETION: u8 = 0;
const WRITE_VALUE: u8 = 1;

/// The tags of the fields of a version edit.
const COMPARATOR: u64 = 1;
const LOG_NUMBER: u64 = 2;
const NEXT_FILE_NUMBER: u64 = 3;
const LAST_SEQUENCE: u64 = 4;
const COMPACT_POINTER: u64 = 5;
const DELETED_FILE: u64 = 6;
const NEW_FILE: u64 = 7;
const PREV_LOG_NUMBER: u64 = 9;

/// A LEB128 number at the start of `bytes`, and the bytes after it.
pub(crate) fn varint(bytes: &[u8]) -> Option<(u64, &[u8])> {
    let mut value = 0_u64;
    for (i, byte) in bytes.iter().enumerate().take(10) {
        value |= u64::from(byte & 0x7F) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((value, &bytes[i + 1..]));
        }
    }
    None
}

/// A byte string written as its length and its bytes, and the bytes after it.
fn slice(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let (len, rest) = varint(bytes)?;
    let len = usize::try_from(len).ok()?;
    Some((rest.get(..len)?, rest.get(len..)?))
}

/// The live files of a database, as its manifest says.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Version {
    pub tables: BTreeSet<u64>,
    /// Logs with this number or a higher one are live.
    pub log_number: u64,
    /// A log being replaced when the manifest was written, also live; 0 for none.
    pub prev_log_number: u64,
}

impl Version {
    fn is_live_log(&self, number: u64) -> bool {
        number >= self.log_number || (self.prev_log_number != 0 && number == self.prev_log_number)
    }

    /// Applies one version edit of the manifest.
    fn apply(&mut self, edit: &[u8]) -> Result<(), String> {
        let broken = || "a version edit of the manifest cannot be read".to_owned();
        let mut rest = edit;
        while !rest.is_empty() {
            let (tag, after) = varint(rest).ok_or_else(broken)?;
            rest = match tag {
                COMPARATOR => slice(after).ok_or_else(broken)?.1,
                LOG_NUMBER | PREV_LOG_NUMBER => {
                    let (number, after) = varint(after).ok_or_else(broken)?;
                    if tag == LOG_NUMBER {
                        self.log_number = number;
                    } else {
                        self.prev_log_number = number;
                    }
                    after
                }
                NEXT_FILE_NUMBER | LAST_SEQUENCE => varint(after).ok_or_else(broken)?.1,
                COMPACT_POINTER => {
                    let (_level, after) = varint(after).ok_or_else(broken)?;
                    slice(after).ok_or_else(broken)?.1
                }
                DELETED_FILE => {
                    let (_level, after) = varint(after).ok_or_else(broken)?;
                    let (number, after) = varint(after).ok_or_else(broken)?;
                    self.tables.remove(&number);
                    after
                }
                NEW_FILE => {
                    let (_level, after) = varint(after).ok_or_else(broken)?;
                    let (number, after) = varint(after).ok_or_else(broken)?;
                    let (_size, after) = varint(after).ok_or_else(broken)?;
                    let (_smallest, after) = slice(after).ok_or_else(broken)?;
                    let (_largest, after) = slice(after).ok_or_else(broken)?;
                    self.tables.insert(number);
                    after
                }
                other => return Err(format!("unknown field {other} in a version edit")),
            };
        }
        Ok(())
    }
}

/// The records of a log (the manifest, or a log of writes), as whole records. A piece cut off at
/// the end of the file ends the records, as `LevelDB` reads a log whose writer stopped or is
/// still writing.
///
/// # Errors
/// What `LevelDB` takes as corruption: a piece whose checksum does not match, a piece of an
/// unknown type, and pieces out of order (a middle or last piece with no first one, or a first
/// or whole record while a record is still open).
fn log_records(bytes: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let out_of_order = || "the pieces of a record of the log are out of order".to_owned();
    let mut records = Vec::new();
    let mut pending: Option<Vec<u8>> = None;
    for block in bytes.chunks(BLOCK_SIZE) {
        let mut rest = block;
        while rest.len() >= HEADER_SIZE {
            let checksum = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]);
            let len = usize::from(u16::from_le_bytes([rest[4], rest[5]]));
            let kind = rest[6];
            if kind == 0 && len == 0 {
                break;
            }
            let Some(data) = rest.get(HEADER_SIZE..HEADER_SIZE + len) else {
                return Ok(records);
            };
            if unmask(checksum) != crc32c::crc32c_append(crc32c::crc32c(&[kind]), data) {
                return Err("a record of the log has a wrong checksum".into());
            }
            match (kind, pending.as_mut()) {
                (FULL, None) => records.push(data.to_vec()),
                (FIRST, None) => pending = Some(data.to_vec()),
                (MIDDLE, Some(record)) => record.extend_from_slice(data),
                (LAST, Some(record)) => {
                    record.extend_from_slice(data);
                    records.extend(pending.take());
                }
                (FULL | FIRST | MIDDLE | LAST, _) => return Err(out_of_order()),
                _ => {
                    return Err(format!("a record of the log has the unknown type {kind}"));
                }
            }
            rest = &rest[HEADER_SIZE + len..];
        }
    }
    Ok(records)
}

/// The writes of one batch of a log of writes: its first sequence number, the count of its
/// entries, then each entry, a deletion (a key) or a value (a key and a value). A batch that does
/// not add up is an error, as `LevelDB` takes it when it recovers its log.
fn batch_records(batch: &[u8], origin: &Path) -> Result<Vec<leveldb_core::Record>, String> {
    let broken = || "a batch of writes of the log cannot be read".to_owned();
    let (seq, rest) = batch.split_first_chunk::<8>().ok_or_else(broken)?;
    let (count, mut rest) = rest.split_first_chunk::<4>().ok_or_else(broken)?;
    let seq = u64::from_le_bytes(*seq);
    let mut records = Vec::new();
    for i in 0..u32::from_le_bytes(*count) {
        let (&kind, after) = rest.split_first().ok_or_else(broken)?;
        let (key, after) = slice(after).ok_or_else(broken)?;
        let (value, after) = match kind {
            WRITE_VALUE => slice(after).ok_or_else(broken)?,
            WRITE_DELETION => (&[][..], after),
            _ => return Err(broken()),
        };
        records.push(leveldb_core::Record {
            key: key.to_vec(),
            value: value.to_vec(),
            seq: seq.checked_add(u64::from(i)).ok_or_else(broken)?,
            deleted: kind == WRITE_DELETION,
            origin_file: origin.to_owned(),
        });
        rest = after;
    }
    if !rest.is_empty() {
        return Err(broken());
    }
    Ok(records)
}

fn unmask(masked: u32) -> u32 {
    masked.wrapping_sub(CRC_MASK_DELTA).rotate_left(15)
}

/// The manifest `CURRENT` names in `dir`: its name, its bytes and the version it describes.
pub(crate) struct Manifest {
    pub name: String,
    pub bytes: Vec<u8>,
    pub version: Version,
}

/// The manifest `dir` is at now.
///
/// # Errors
/// Fails when `CURRENT` or the manifest cannot be read, or the manifest holds an edit this
/// reader does not know.
pub(crate) fn current_manifest(dir: &Path) -> Result<Manifest, Error> {
    let current = dir.join("CURRENT");
    let name = fs::read_to_string(&current)
        .map_err(|e| Error::Unreadable(current.clone(), e.to_string()))?
        .trim_end()
        .to_owned();
    if !name.starts_with("MANIFEST-") || name.contains(['/', '\\']) {
        return Err(Error::Unreadable(
            current,
            format!("names no manifest: {name}"),
        ));
    }
    let path = dir.join(&name);
    let bytes = fs::read(&path).map_err(|e| Error::Unreadable(path.clone(), e.to_string()))?;
    let mut version = Version::default();
    let edits = log_records(&bytes).map_err(|reason| Error::Unreadable(path.clone(), reason))?;
    for edit in edits {
        version
            .apply(&edit)
            .map_err(|reason| Error::Unreadable(path.clone(), reason))?;
    }
    Ok(Manifest {
        name,
        bytes,
        version,
    })
}

/// The live files of `dir` for `version`: every live table, and every log in the directory
/// that is live.
///
/// # Errors
/// Fails when the directory cannot be listed.
pub(crate) fn live_files(dir: &Path, version: &Version) -> Result<Vec<PathBuf>, Error> {
    let mut files: Vec<PathBuf> = version
        .tables
        .iter()
        .map(|number| {
            let table = dir.join(format!("{number:06}.ldb"));
            let old_name = dir.join(format!("{number:06}.sst"));
            if !table.exists() && old_name.exists() {
                old_name
            } else {
                table
            }
        })
        .collect();
    let entries =
        fs::read_dir(dir).map_err(|e| Error::Unreadable(dir.to_owned(), e.to_string()))?;
    for entry in entries {
        let path = entry
            .map_err(|e| Error::Unreadable(dir.to_owned(), e.to_string()))?
            .path();
        let log_number = path
            .extension()
            .filter(|e| *e == "log")
            .and(path.file_stem())
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.parse::<u64>().ok());
        if log_number.is_some_and(|number| version.is_live_log(number)) {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Every record of the live files of the database in `dir`.
///
/// # Errors
/// Fails when the manifest or a live file cannot be read; a live table that is not there or
/// does not parse is an error, not a gap, and so is a damaged record of a log anywhere but at
/// its end, where the writer may still be writing.
pub(crate) fn read_live_records(dir: &Path) -> Result<Vec<leveldb_core::Record>, Error> {
    let manifest = current_manifest(dir)?;
    let mut records = Vec::new();
    for path in live_files(dir, &manifest.version)? {
        let unreadable = |reason: String| Error::Unreadable(path.clone(), reason);
        let bytes = fs::read(&path).map_err(|e| unreadable(e.to_string()))?;
        if path.extension().is_some_and(|e| e == "log") {
            for batch in log_records(&bytes).map_err(unreadable)? {
                records.extend(batch_records(&batch, &path).map_err(unreadable)?);
            }
        } else {
            let table = leveldb_core::parse_table_bytes(&bytes, &path);
            records.extend(table.map_err(|e| unreadable(e.to_string()))?);
        }
    }
    Ok(records)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn put_varint(out: &mut Vec<u8>, mut value: u64) {
        loop {
            let byte = (value & 0x7F) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }

    /// A version edit that sets the log number and adds the tables `added`, as `LevelDB`
    /// writes it.
    pub(crate) fn edit(log_number: u64, added: &[u64], deleted: &[u64]) -> Vec<u8> {
        let mut out = Vec::new();
        put_varint(&mut out, COMPARATOR);
        put_varint(&mut out, 8);
        out.extend_from_slice(b"idb_cmp1");
        put_varint(&mut out, LOG_NUMBER);
        put_varint(&mut out, log_number);
        for number in added {
            put_varint(&mut out, NEW_FILE);
            put_varint(&mut out, 0);
            put_varint(&mut out, *number);
            put_varint(&mut out, 100);
            for key in [b"a".as_slice(), b"z"] {
                put_varint(&mut out, key.len() as u64);
                out.extend_from_slice(key);
            }
        }
        for number in deleted {
            put_varint(&mut out, DELETED_FILE);
            put_varint(&mut out, 0);
            put_varint(&mut out, *number);
        }
        out
    }

    /// Records in a log file, each as one piece.
    pub(crate) fn log(records: &[Vec<u8>]) -> Vec<u8> {
        let full: Vec<(u8, &[u8])> = records.iter().map(|r| (FULL, r.as_slice())).collect();
        pieces(&full)
    }

    /// Pieces of log records, each with its type.
    fn pieces(pieces: &[(u8, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (kind, data) in pieces {
            let crc = crc32c::crc32c_append(crc32c::crc32c(&[*kind]), data);
            let masked = crc.rotate_right(15).wrapping_add(CRC_MASK_DELTA);
            out.extend(masked.to_le_bytes());
            out.extend(u16::try_from(data.len()).unwrap().to_le_bytes());
            out.push(*kind);
            out.extend_from_slice(data);
        }
        out
    }

    #[test]
    fn a_record_in_pieces_is_joined_and_pieces_out_of_order_are_an_error() {
        let joined = pieces(&[(FIRST, b"ab"), (MIDDLE, b"cd"), (LAST, b"ef"), (FULL, b"g")]);
        assert_eq!(
            log_records(&joined),
            Ok(vec![b"abcdef".to_vec(), b"g".to_vec()])
        );
        let out_of_order = Err("the pieces of a record of the log are out of order".into());
        assert_eq!(log_records(&pieces(&[(LAST, b"x")])), out_of_order);
        assert_eq!(log_records(&pieces(&[(MIDDLE, b"x")])), out_of_order);
        assert_eq!(
            log_records(&pieces(&[(FIRST, b"a"), (FULL, b"b")])),
            out_of_order
        );
    }

    #[test]
    fn the_edits_of_a_manifest_give_the_live_tables_and_logs() {
        let mut version = Version::default();
        for edit in log_records(&log(&[edit(3, &[5, 7], &[]), edit(8, &[9], &[5])])).unwrap() {
            version.apply(&edit).unwrap();
        }
        assert_eq!(
            version,
            Version {
                tables: BTreeSet::from([7, 9]),
                log_number: 8,
                prev_log_number: 0,
            }
        );
        assert!(!version.is_live_log(3) && version.is_live_log(8) && version.is_live_log(10));
    }

    #[test]
    fn a_manifest_cut_off_ends_at_its_last_whole_edit_and_a_bad_checksum_is_an_error() {
        let whole = log(&[edit(3, &[5], &[]), edit(8, &[9], &[])]);
        assert_eq!(log_records(&whole).unwrap().len(), 2);
        assert_eq!(log_records(&whole[..whole.len() - 1]).unwrap().len(), 1);
        let mut flipped = whole.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 0xFF;
        assert_eq!(
            log_records(&flipped),
            Err("a record of the log has a wrong checksum".to_owned())
        );
    }

    /// A block of entries, none sharing a prefix, with one restart point.
    fn block(entries: &[(Vec<u8>, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (key, value) in entries {
            put_varint(&mut out, 0);
            put_varint(&mut out, key.len() as u64);
            put_varint(&mut out, value.len() as u64);
            out.extend_from_slice(key);
            out.extend_from_slice(value);
        }
        out.extend(0_u32.to_le_bytes());
        out.extend(1_u32.to_le_bytes());
        out
    }

    /// Appends an uncompressed block with its trailer; its handle (offset, size).
    fn put_block(table: &mut Vec<u8>, block: &[u8]) -> Vec<u8> {
        let offset = table.len();
        table.extend_from_slice(block);
        table.push(0);
        let crc = crc32c::crc32c(&table[offset..]);
        table.extend(
            crc.rotate_right(15)
                .wrapping_add(CRC_MASK_DELTA)
                .to_le_bytes(),
        );
        let mut handle = Vec::new();
        put_varint(&mut handle, offset as u64);
        put_varint(&mut handle, block.len() as u64);
        handle
    }

    /// A table file of one data block, as `LevelDB` writes it: values `(key, sequence, value)`
    /// in key order.
    fn table(values: &[(&[u8], u64, &[u8])]) -> Vec<u8> {
        let entries: Vec<(Vec<u8>, &[u8])> = values
            .iter()
            .map(|(key, seq, value)| {
                let trailer = (seq << 8) | u64::from(WRITE_VALUE);
                ([*key, &trailer.to_le_bytes()].concat(), *value)
            })
            .collect();
        let mut out = Vec::new();
        let data = put_block(&mut out, &block(&entries));
        let meta = put_block(&mut out, &block(&[]));
        let last = entries
            .last()
            .map(|(key, _)| key.clone())
            .unwrap_or_default();
        let index = put_block(&mut out, &block(&[(last, &data)]));
        let mut footer = [meta, index].concat();
        footer.resize(40, 0);
        footer.extend(0xdb47_7524_8b80_fb57_u64.to_le_bytes());
        out.extend(footer);
        out
    }

    /// A batch of writes of a log, from the sequence number `seq`.
    fn batch(seq: u64, writes: &[(&[u8], Option<&[u8]>)]) -> Vec<u8> {
        let mut out = seq.to_le_bytes().to_vec();
        out.extend(u32::try_from(writes.len()).unwrap().to_le_bytes());
        for (key, value) in writes {
            out.push(if value.is_some() {
                WRITE_VALUE
            } else {
                WRITE_DELETION
            });
            put_varint(&mut out, key.len() as u64);
            out.extend_from_slice(key);
            if let Some(value) = value {
                put_varint(&mut out, value.len() as u64);
                out.extend_from_slice(value);
            }
        }
        out
    }

    #[test]
    fn the_live_tables_and_logs_are_read_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("leveldb-tables-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Table 3 was compacted away; 4 is live; 6 is live under the old `.sst` name. Log 2 is
        // older than the log number; log 7 is live.
        let manifest = log(&[edit(2, &[3], &[]), edit(7, &[4, 6], &[3])]);
        fs::write(dir.join("MANIFEST-000001"), manifest).unwrap();
        fs::write(dir.join("CURRENT"), "MANIFEST-000001\n").unwrap();
        fs::write(dir.join("000003.ldb"), table(&[(b"stale", 1, b"gone")])).unwrap();
        let live = table(&[(b"a", 2, b"from a table"), (b"b", 3, b"old")]);
        fs::write(dir.join("000004.ldb"), live).unwrap();
        fs::write(dir.join("000006.sst"), table(&[(b"c", 4, b"old name")])).unwrap();
        let old_log = log(&[batch(5, &[(b"x", Some(b"old log"))])]);
        fs::write(dir.join("000002.log"), old_log).unwrap();
        let writes = batch(10, &[(b"b", Some(b"new")), (b"c", None)]);
        fs::write(dir.join("000007.log"), log(&[writes])).unwrap();

        let mut records = read_live_records(&dir).unwrap();
        records.sort_by(|x, y| (&x.key, x.seq).cmp(&(&y.key, y.seq)));
        let seen: Vec<(&[u8], u64, bool, &[u8])> = records
            .iter()
            .map(|r| (r.key.as_slice(), r.seq, r.deleted, r.value.as_slice()))
            .collect();
        assert_eq!(
            seen,
            [
                (b"a".as_slice(), 2, false, b"from a table".as_slice()),
                (b"b", 3, false, b"old"),
                (b"b", 10, false, b"new"),
                (b"c", 4, false, b"old name"),
                (b"c", 11, true, b""),
            ]
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_damaged_record_or_batch_in_a_log_is_an_error() {
        let dir = std::env::temp_dir().join(format!("leveldb-damaged-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("MANIFEST-000001"), log(&[edit(7, &[], &[])])).unwrap();
        fs::write(dir.join("CURRENT"), "MANIFEST-000001\n").unwrap();
        let first = batch(1, &[(b"a", Some(b"1"))]);
        let mut damaged = log(&[first.clone(), batch(2, &[(b"b", Some(b"2"))])]);
        damaged[HEADER_SIZE + 2] ^= 0xFF;
        fs::write(dir.join("000007.log"), damaged).unwrap();
        let error = read_live_records(&dir).unwrap_err().to_string();
        assert!(
            error.contains("a record of the log has a wrong checksum"),
            "{error}"
        );
        // A batch that says it holds two writes and holds one, and one that holds more than it
        // says.
        let mut short = first;
        short[8] = 2;
        let mut long = batch(1, &[(b"a", Some(b"1")), (b"b", Some(b"2"))]);
        long[8] = 1;
        for wrong in [short, long] {
            fs::write(dir.join("000007.log"), log(&[wrong])).unwrap();
            let error = read_live_records(&dir).unwrap_err().to_string();
            assert!(
                error.contains("a batch of writes of the log cannot be read"),
                "{error}"
            );
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn an_edit_with_an_unknown_field_is_refused() {
        let mut version = Version::default();
        assert_eq!(
            version.apply(&[0x63, 0x01]),
            Err("unknown field 99 in a version edit".to_owned())
        );
    }
}
