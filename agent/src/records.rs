//! Where the records read from the coding tools' session logs are kept: JSON lines in
//! `records/<tool>/<session>.jsonl` in the agent's data directory, plus the read offset and
//! per-file state of every log file. Plain files, no database.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::hash::Hasher;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use connectors::{compare_times, is_utc_time, minute_of, FileState, Record, Role, Turn};
use fnv::FnvHasher;
use serde::{Deserialize, Serialize};

const PROGRESS_FILE: &str = "connectors-progress.json";
const MAX_PLAIN_NAME: usize = 64;
const HASH_PREFIX: &str = "h-";

/// Where a log file was read up to, and what the reader learned from it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileProgress {
    pub offset: u64,
    pub state: FileState,
}

/// The progress of every log file, by path.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    pub files: BTreeMap<String, FileProgress>,
}

/// What is known about one session file after it was first touched in this run.
struct SessionFile {
    /// The minutes that have a mark.
    marked: HashSet<String>,
    /// Keys of the stored turns.
    turns: HashSet<u64>,
}

impl SessionFile {
    fn has(&self, record: &Record) -> bool {
        match record {
            Record::Mark(m) => self.marked.contains(minute_of(&m.time)),
            Record::Turn(t) => self.turns.contains(&turn_key(t)),
        }
    }

    fn note(&mut self, record: &Record) {
        match record {
            Record::Mark(m) => {
                self.marked.insert(minute_of(&m.time).to_owned());
            }
            Record::Turn(t) => {
                self.turns.insert(turn_key(t));
            }
        }
    }
}

/// What the readers write to and the rest of the agent reads from: records on plain files
/// under a data directory.
///
/// The sink keeps what it knows about each session file in memory, so it is correct only while
/// it is the one writer of its directory. The agent holds the lock of its data directory for
/// its whole run (see `database`); open a sink only while holding it.
pub struct FileSink {
    dir: PathBuf,
    sessions: HashMap<PathBuf, SessionFile>,
    /// Files written since the last `save_progress`, which makes them durable.
    unsynced: HashSet<PathBuf>,
}

impl FileSink {
    /// A sink under `data_dir`; nothing is created until the first write.
    #[must_use]
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.to_owned(),
            sessions: HashMap::new(),
            unsynced: HashSet::new(),
        }
    }

    /// The directory of one tool's session files.
    fn tool_dir(&self, tool: &str) -> PathBuf {
        self.dir.join("records").join(file_name(tool))
    }

    fn session_path(&self, tool: &str, session: &str) -> PathBuf {
        self.tool_dir(tool)
            .join(format!("{}.jsonl", file_name(session)))
    }

    /// Adds a record. A mark is dropped when its session already has one in the same minute,
    /// and a turn when the same turn (time, role, text) is stored already, so a replay after
    /// a crash adds nothing. The record is on the disk once `save_progress` returned.
    ///
    /// # Errors
    /// Fails when the time of the record is not a UTC RFC 3339 time (`2026-11-11T10:15:30Z`,
    /// optionally with a fraction of a second), when a stored line is damaged, or when the
    /// file cannot be written.
    pub fn append(&mut self, record: &Record) -> io::Result<()> {
        if !is_utc_time(record.time()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("not a UTC RFC 3339 time: {}", record.time()),
            ));
        }
        let tool_dir = self.tool_dir(record.tool());
        let path = self.session_path(record.tool(), record.session());
        let known = match self.sessions.entry(path.clone()) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(e) => e.insert(load_session(&path)?),
        };
        // A replay after a crash (the offset was saved before the last records) re-sends
        // what is stored already: drop it, so nothing is stored twice.
        if known.has(record) {
            return Ok(());
        }
        let mut line = serde_json::to_string(record).map_err(io::Error::other)?;
        line.push('\n');
        // One write for the line and its newline, so lines of one file never interleave.
        let written = fs::create_dir_all(&tool_dir).and_then(|()| {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?
                .write_all(line.as_bytes())
        });
        if let Err(e) = written {
            // A failed write may have left half a line: forget the file, so the next append
            // reads it again and cuts the half line off.
            self.sessions.remove(&path);
            return Err(e);
        }
        known.note(record);
        self.unsynced.insert(path);
        Ok(())
    }

    /// The last `limit` turns of a session before `before` (UTC RFC 3339), oldest first.
    ///
    /// # Errors
    /// Fails when the file cannot be read or a stored line is damaged.
    pub fn turns_before(
        &self,
        tool: &str,
        session: &str,
        before: &str,
        limit: usize,
    ) -> io::Result<Vec<Turn>> {
        let (records, _) = read_records(&self.session_path(tool, session))?;
        let mut turns: Vec<Turn> = records
            .into_iter()
            .filter_map(|r| match r {
                Record::Turn(t) if compare_times(&t.time, before).is_lt() => Some(t),
                _ => None,
            })
            .collect();
        let excess = turns.len().saturating_sub(limit);
        turns.drain(..excess);
        Ok(turns)
    }

    /// Makes the appended records durable, then saves the progress of every log file
    /// atomically (temporary file, rename, directory sync).
    ///
    /// # Errors
    /// Fails when the file cannot be written.
    pub fn save_progress(&mut self, progress: &Progress) -> io::Result<()> {
        // The offset may only move past records that are on the disk.
        for path in &self.unsynced {
            OpenOptions::new().append(true).open(path)?.sync_data()?;
            // A first write also created `records/<tool>` and maybe `records/`: make the whole
            // chain of new entries durable, up to the data directory (synced below).
            for dir in path.ancestors().skip(1) {
                sync_dir(dir)?;
                if dir == self.dir.join("records") {
                    break;
                }
            }
        }
        self.unsynced.clear();
        fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!("{PROGRESS_FILE}.tmp"));
        let mut file = File::create(&tmp)?;
        file.write_all(&serde_json::to_vec(progress).map_err(io::Error::other)?)?;
        file.sync_all()?;
        fs::rename(&tmp, self.dir.join(PROGRESS_FILE))?;
        sync_dir(&self.dir)
    }

    /// The saved progress; empty when nothing was saved yet.
    ///
    /// # Errors
    /// Fails when the file is there but cannot be read or parsed.
    pub fn load_progress(&self) -> io::Result<Progress> {
        match fs::read(self.dir.join(PROGRESS_FILE)) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Progress::default()),
            Err(e) => Err(e),
        }
    }
}

/// FNV-1a over the parts, each followed by a zero byte. The algorithm is fixed by its
/// definition, unlike the standard hasher, so names and keys stay stable.
fn fnv(parts: &[&str]) -> u64 {
    let mut hasher = FnvHasher::default();
    for part in parts {
        hasher.write(part.as_bytes());
        hasher.write_u8(0);
    }
    hasher.finish()
}

/// A name that can only be a file name: short ids of lower-case letters, digits, `_` and `-`
/// as they are, anything else as a hash, so an id can never leave the directory. Upper case
/// is hashed too (ids differing only by case must not share a file on a case-insensitive
/// disk), and so is anything that looks like a hashed name, so a hash is never impersonated.
fn file_name(id: &str) -> String {
    let plain = !id.is_empty()
        && id.len() <= MAX_PLAIN_NAME
        && !id.starts_with(HASH_PREFIX)
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if plain {
        return id.to_owned();
    }
    format!("{HASH_PREFIX}{:016x}", fnv(&[id]))
}

/// The identity of a stored turn, for dropping a replayed one.
fn turn_key(t: &Turn) -> u64 {
    let role = match t.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    fnv(&[&t.time, role, &t.text])
}

/// The records of a file and the length of its complete lines. Bytes after the last newline
/// are a line cut short by a crash: they are left out. Any other line that does not parse is an
/// error, so damage in the middle of a file is never taken for a missing record.
fn read_records(path: &Path) -> io::Result<(Vec<Record>, u64)> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(e) => return Err(e),
    };
    let complete = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    let mut records = Vec::new();
    for (number, line) in bytes[..complete]
        .split_inclusive(|b| *b == b'\n')
        .enumerate()
    {
        let record = serde_json::from_slice(line).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: line {}: {e}", path.display(), number + 1),
            )
        })?;
        records.push(record);
    }
    Ok((records, complete as u64))
}

/// What a session file holds. A line cut short by a crash is cut off the file: its offset was
/// never saved, so the reader sends it again.
fn load_session(path: &Path) -> io::Result<SessionFile> {
    let (records, complete) = read_records(path)?;
    if fs::metadata(path).is_ok_and(|m| m.len() > complete) {
        OpenOptions::new()
            .write(true)
            .open(path)?
            .set_len(complete)?;
    }
    let mut known = SessionFile {
        marked: HashSet::new(),
        turns: HashSet::new(),
    };
    for r in &records {
        known.note(r);
    }
    Ok(known)
}

/// Makes a created or renamed entry of a directory durable. Windows cannot open a directory
/// for this and keeps such entries durable on its own.
fn sync_dir(dir: &Path) -> io::Result<()> {
    if cfg!(unix) {
        File::open(dir)?.sync_all()
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use connectors::{Mark, Role};

    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("records-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn turn(tool: &str, session: &str, time: &str, text: &str) -> Record {
        Record::Turn(Turn {
            tool: tool.into(),
            session: session.into(),
            time: time.into(),
            folder: "/work".into(),
            branch: String::new(),
            role: Role::User,
            text: text.into(),
        })
    }

    fn mark(tool: &str, session: &str, time: &str) -> Record {
        Record::Mark(Mark {
            tool: tool.into(),
            session: session.into(),
            time: time.into(),
            folder: "/work".into(),
            branch: "main".into(),
        })
    }

    fn texts(turns: &[Turn]) -> Vec<&str> {
        turns.iter().map(|t| t.text.as_str()).collect()
    }

    #[test]
    fn appends_and_reads_back_in_order() {
        let mut sink = FileSink::new(&temp("order"));
        for (i, t) in ["10:00:01", "10:00:02", "10:00:03", "10:00:04"]
            .iter()
            .enumerate()
        {
            let time = format!("2026-11-11T{t}Z");
            sink.append(&turn("claude", "s1", &time, &i.to_string()))
                .unwrap();
        }
        let all = sink
            .turns_before("claude", "s1", "2026-11-11T11:00:00Z", 10)
            .unwrap();
        assert_eq!(texts(&all), ["0", "1", "2", "3"]);
        let before = sink
            .turns_before("claude", "s1", "2026-11-11T10:00:03Z", 10)
            .unwrap();
        assert_eq!(texts(&before), ["0", "1"]);
        let limited = sink
            .turns_before("claude", "s1", "2026-11-11T11:00:00Z", 2)
            .unwrap();
        assert_eq!(texts(&limited), ["2", "3"]);
    }

    #[test]
    fn sessions_and_tools_do_not_mix() {
        let mut sink = FileSink::new(&temp("mix"));
        let t = "2026-11-11T10:00:00Z";
        sink.append(&turn("claude", "s1", t, "a")).unwrap();
        sink.append(&turn("claude", "s2", t, "b")).unwrap();
        sink.append(&turn("codex", "s1", t, "c")).unwrap();
        let end = "2026-11-12T00:00:00Z";
        assert_eq!(
            texts(&sink.turns_before("claude", "s1", end, 9).unwrap()),
            ["a"]
        );
        assert_eq!(
            texts(&sink.turns_before("claude", "s2", end, 9).unwrap()),
            ["b"]
        );
        assert_eq!(
            texts(&sink.turns_before("codex", "s1", end, 9).unwrap()),
            ["c"]
        );
        assert!(sink.turns_before("codex", "s2", end, 9).unwrap().is_empty());
    }

    #[test]
    fn second_mark_of_a_minute_is_dropped_even_after_reopen() {
        let dir = temp("marks");
        let mut sink = FileSink::new(&dir);
        sink.append(&mark("claude", "s1", "2026-11-11T10:00:05Z"))
            .unwrap();
        sink.append(&mark("claude", "s1", "2026-11-11T10:00:50Z"))
            .unwrap();
        sink.append(&mark("claude", "s1", "2026-11-11T10:01:00Z"))
            .unwrap();
        sink.append(&mark("claude", "s2", "2026-11-11T10:00:50Z"))
            .unwrap();
        let mut reopened = FileSink::new(&dir);
        reopened
            .append(&mark("claude", "s1", "2026-11-11T10:01:30Z"))
            .unwrap();
        let count = |s: &str| {
            let path = dir.join("records/claude").join(format!("{s}.jsonl"));
            fs::read_to_string(path).unwrap().lines().count()
        };
        assert_eq!(count("s1"), 2);
        assert_eq!(count("s2"), 1);
    }

    #[test]
    fn progress_is_saved_atomically_and_survives_reopen() {
        let dir = temp("progress");
        let mut sink = FileSink::new(&dir);
        assert_eq!(sink.load_progress().unwrap(), Progress::default());
        let mut progress = Progress::default();
        progress.files.insert(
            "/logs/a.jsonl".into(),
            FileProgress {
                offset: 42,
                state: FileState {
                    session: "s1".into(),
                    folder: "/work".into(),
                    branch: "main".into(),
                },
            },
        );
        sink.save_progress(&progress).unwrap();
        // The temporary file is gone: it was renamed over the real one.
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, [PROGRESS_FILE]);
        assert_eq!(FileSink::new(&dir).load_progress().unwrap(), progress);
        progress.files.clear();
        sink.save_progress(&progress).unwrap();
        assert_eq!(FileSink::new(&dir).load_progress().unwrap(), progress);
    }

    #[test]
    fn torn_last_line_is_ignored_and_cut_off_before_the_next_write() {
        let dir = temp("torn");
        let mut sink = FileSink::new(&dir);
        sink.append(&turn("claude", "s1", "2026-11-11T10:00:00Z", "a"))
            .unwrap();
        let path = dir.join("records/claude/s1.jsonl");
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(br#"{"kind":"turn","tool":"cla"#).unwrap();
        let end = "2026-11-12T00:00:00Z";
        assert_eq!(
            texts(&sink.turns_before("claude", "s1", end, 9).unwrap()),
            ["a"]
        );
        let mut reopened = FileSink::new(&dir);
        reopened
            .append(&turn("claude", "s1", "2026-11-11T10:00:01Z", "b"))
            .unwrap();
        assert_eq!(
            texts(&reopened.turns_before("claude", "s1", end, 9).unwrap()),
            ["a", "b"]
        );
        // Every line of the file is whole: the half line is gone, not kept in the middle.
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text
            .lines()
            .all(|l| serde_json::from_str::<Record>(l).is_ok()));
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_write_does_not_glue_the_next_record_to_a_half_line() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp("failedwrite");
        let mut sink = FileSink::new(&dir);
        sink.append(&turn("claude", "s1", "2026-11-11T10:00:00Z", "a"))
            .unwrap();
        let path = dir.join("records/claude/s1.jsonl");
        // The file cannot be opened for writing: the append fails.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        let failed = sink.append(&turn("claude", "s1", "2026-11-11T10:00:01Z", "b"));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        if failed.is_ok() {
            return; // running as a user who ignores file permissions
        }
        // Meanwhile a half written line landed in the file.
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(br#"{"kind":"turn","tool":"cla"#).unwrap();
        sink.append(&turn("claude", "s1", "2026-11-11T10:00:02Z", "c"))
            .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text
            .lines()
            .all(|l| serde_json::from_str::<Record>(l).is_ok()));
    }

    #[test]
    fn a_damaged_line_in_the_middle_is_an_error() {
        let dir = temp("damaged");
        let mut sink = FileSink::new(&dir);
        sink.append(&turn("claude", "s1", "2026-11-11T10:00:00Z", "a"))
            .unwrap();
        let path = dir.join("records/claude/s1.jsonl");
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"{\"kind\":\"from a newer version\"}\n")
            .unwrap();
        let end = "2026-11-12T00:00:00Z";
        let err = sink.turns_before("claude", "s1", end, 9).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("line 2"), "{err}");
        let err = FileSink::new(&dir)
            .append(&turn("claude", "s1", "2026-11-11T10:00:01Z", "b"))
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn a_time_with_an_offset_is_refused() {
        let mut sink = FileSink::new(&temp("offset"));
        let err = sink
            .append(&turn("claude", "s1", "2026-11-11T12:00:00+02:00", "a"))
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn a_corrupt_progress_file_is_an_error() {
        let dir = temp("badprogress");
        fs::write(dir.join(PROGRESS_FILE), b"{not json").unwrap();
        assert!(FileSink::new(&dir).load_progress().is_err());
    }

    #[test]
    fn replaying_a_batch_stores_nothing_twice() {
        let dir = temp("replay");
        let mut sink = FileSink::new(&dir);
        let batch = [
            mark("claude", "s1", "2026-11-11T10:01:05Z"),
            turn("claude", "s1", "2026-11-11T10:01:06Z", "a"),
            mark("claude", "s1", "2026-11-11T10:02:05Z"),
            turn("claude", "s1", "2026-11-11T10:02:06Z", "b"),
        ];
        for r in &batch {
            sink.append(r).unwrap();
        }
        // A crash before the offset was saved: a new run reads the same lines again.
        let mut again = FileSink::new(&dir);
        for r in &batch {
            again.append(r).unwrap();
        }
        let path = dir.join("records/claude/s1.jsonl");
        assert_eq!(fs::read_to_string(path).unwrap().lines().count(), 4);
        let end = "2026-11-12T00:00:00Z";
        assert_eq!(
            texts(&again.turns_before("claude", "s1", end, 9).unwrap()),
            ["a", "b"]
        );
    }

    #[test]
    fn saving_progress_forgets_the_files_it_synced() {
        let dir = temp("sync");
        let mut sink = FileSink::new(&dir);
        sink.append(&turn("claude", "s1", "2026-11-11T10:00:00Z", "a"))
            .unwrap();
        assert_eq!(sink.unsynced.len(), 1);
        sink.save_progress(&Progress::default()).unwrap();
        assert!(sink.unsynced.is_empty());
        assert!(dir.join(PROGRESS_FILE).exists());
    }

    #[test]
    fn names_are_plain_or_a_hash_that_no_id_can_imitate() {
        assert_eq!(file_name("abc-def_09"), "abc-def_09");
        let hashed = file_name("a/b");
        assert!(hashed.starts_with("h-") && hashed.len() == 18);
        // An id that already looks like a hashed name is hashed itself.
        assert_ne!(file_name(&hashed), hashed);
        // Ids that differ only by case do not share a file.
        assert_ne!(file_name("Abc"), file_name("abc"));
        for bad in ["../../etc/passwd", "a/b", "", "a.b", "..", &"x".repeat(200)] {
            let name = file_name(bad);
            assert!(name.starts_with("h-") && name.len() == 18, "{name}");
        }
        assert_ne!(file_name("a/b"), file_name("a/c"));
    }

    #[test]
    fn names_and_keys_are_pinned() {
        // Changing the hash renames every stored file and re-stores every turn.
        assert_eq!(fnv(&["a/b"]), 8_249_156_916_594_893_267);
        assert_eq!(file_name("a/b"), "h-727ae4827e1861d3");
        let mut t = Turn {
            tool: "t".into(),
            session: "s".into(),
            time: "2026-11-11T10:00:00Z".into(),
            folder: "/f".into(),
            branch: String::new(),
            role: Role::User,
            text: "hi".into(),
        };
        assert_eq!(turn_key(&t), 9_679_209_057_029_614_810);
        t.role = Role::Assistant;
        assert_eq!(turn_key(&t), 16_283_009_973_638_215_449);
    }

    #[test]
    fn session_ids_cannot_leave_the_directory() {
        let dir = temp("escape");
        let mut sink = FileSink::new(&dir);
        let id = "../../x";
        sink.append(&turn("claude", id, "2026-11-11T10:00:00Z", "a"))
            .unwrap();
        // From `records/claude/` the id would have landed here.
        assert!(!dir.join("x.jsonl").exists());
        let files: Vec<_> = fs::read_dir(dir.join("records/claude"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(files, [format!("{}.jsonl", file_name(id))]);
        assert!(files[0].starts_with("h-"));
        let found = sink
            .turns_before("claude", id, "2026-11-12T00:00:00Z", 9)
            .unwrap();
        assert_eq!(found.len(), 1);
    }
}
