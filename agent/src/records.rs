//! Where the records read from the coding tools' session logs are kept: JSON lines in
//! `records/<tool>/<session>.jsonl` in the agent's data directory, plus the read offset and
//! per-file state of every log file. Plain files, no database.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use connectors::{FileState, Record, Role, Turn, compare_times, minute_of};
use serde::{Deserialize, Serialize};

use crate::ipc::absolute_env_path;

const PROGRESS_FILE: &str = "connectors-progress.json";
const MAX_PLAIN_NAME: usize = 64;
const HASH_PREFIX: &str = "h-";

/// `$XDG_DATA_HOME/construct` (default `~/.local/share/construct`), the same root as the
/// agent's database.
///
/// # Errors
/// Fails when `HOME` is not set.
pub fn data_dir() -> io::Result<PathBuf> {
    let data_home = match absolute_env_path("XDG_DATA_HOME") {
        Some(dir) => dir,
        None => absolute_env_path("HOME")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?
            .join(".local")
            .join("share"),
    };
    Ok(data_home.join("construct"))
}

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

/// What the readers write to and the rest of the agent reads from.
pub trait Sink {
    /// Adds a record. A mark is dropped when its session already has one in the same minute,
    /// and a turn when the same turn (time, role, text) is stored already, so a replay after
    /// a crash adds nothing. The record is on the disk once `save_progress` returned.
    /// Returns whether the record was stored, `false` for a dropped one.
    ///
    /// # Errors
    /// Fails when the file cannot be written.
    fn append(&mut self, record: &Record) -> io::Result<bool>;

    /// The last `limit` turns of a session before `before` (UTC RFC 3339), oldest first.
    ///
    /// # Errors
    /// Fails when the file cannot be read.
    fn turns_before(
        &self,
        tool: &str,
        session: &str,
        before: &str,
        limit: usize,
    ) -> io::Result<Vec<Turn>>;

    /// Makes the appended records durable, then saves the progress of every log file
    /// atomically (temporary file, rename, directory sync).
    ///
    /// # Errors
    /// Fails when the file cannot be written.
    fn save_progress(&mut self, progress: &Progress) -> io::Result<()>;

    /// The saved progress; empty when nothing was saved yet.
    ///
    /// # Errors
    /// Fails when the file is there but cannot be read or parsed.
    fn load_progress(&self) -> io::Result<Progress>;
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

/// [`Sink`] on plain files under a data directory.
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

    fn session_path(&self, tool: &str, session: &str) -> PathBuf {
        self.dir
            .join("records")
            .join(file_name(tool))
            .join(format!("{}.jsonl", file_name(session)))
    }
}

/// FNV-1a: fixed by definition, unlike the standard hasher, so names and keys stay stable.
fn fnv(parts: &[&str]) -> u64 {
    parts
        .iter()
        .flat_map(|p| p.bytes().chain(std::iter::once(0)))
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        })
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

/// The records of a file; a line that does not parse (a torn last line) is skipped.
fn read_records(path: &Path) -> io::Result<Vec<Record>> {
    let mut text = String::new();
    match File::open(path) {
        Ok(mut f) => {
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes)?;
            text.push_str(&String::from_utf8_lossy(&bytes));
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    Ok(text
        .lines()
        .filter_map(|l| serde_json::from_str::<Record>(l).ok())
        .collect())
}

/// Whether a file is empty or ends with a newline.
fn ends_cleanly(file: &mut File) -> io::Result<bool> {
    let len = file.metadata()?.len();
    if len == 0 {
        return Ok(true);
    }
    file.seek(SeekFrom::Start(len - 1))?;
    let mut last = [0_u8; 1];
    file.read_exact(&mut last)?;
    Ok(last[0] == b'\n')
}

/// Options for a file only the user can read: what the tools said is copied here. The mode
/// only counts for a file that is created.
fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
}

/// Creates `dir` and every directory from `data_dir` down to it as user-only.
fn private_dirs(data_dir: &Path, dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let mut chain: Vec<&Path> = dir
            .ancestors()
            .take_while(|d| d.starts_with(data_dir))
            .collect();
        chain.reverse();
        for d in chain {
            crate::ipc::create_private_dir(d)?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = data_dir;
        fs::create_dir_all(dir)
    }
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

impl Sink for FileSink {
    fn append(&mut self, record: &Record) -> io::Result<bool> {
        let path = self.session_path(record.tool(), record.session());
        if !self.sessions.contains_key(&path) {
            let mut known = SessionFile {
                marked: HashSet::new(),
                turns: HashSet::new(),
            };
            for r in read_records(&path)? {
                known.note(&r);
            }
            self.sessions.insert(path.clone(), known);
        }
        // A replay after a crash (the offset was saved before the last records) re-sends
        // what is stored already: drop it, so nothing is stored twice.
        if self.sessions.get(&path).is_some_and(|k| k.has(record)) {
            return Ok(false);
        }
        if let Some(parent) = path.parent() {
            private_dirs(&self.dir, parent)?;
        }
        let mut file = private_options()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;
        let mut line = String::new();
        // A crash can leave a half line; start on a fresh line so it stays the only damage.
        if !ends_cleanly(&mut file)? {
            line.push('\n');
        }
        line.push_str(&serde_json::to_string(record).map_err(io::Error::other)?);
        line.push('\n');
        // One write for the line and its newline, so lines of one file never interleave.
        file.write_all(line.as_bytes())?;
        self.unsynced.insert(path.clone());
        if let Some(known) = self.sessions.get_mut(&path) {
            known.note(record);
        }
        Ok(true)
    }

    fn turns_before(
        &self,
        tool: &str,
        session: &str,
        before: &str,
        limit: usize,
    ) -> io::Result<Vec<Turn>> {
        let mut turns: Vec<Turn> = read_records(&self.session_path(tool, session))?
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

    fn save_progress(&mut self, progress: &Progress) -> io::Result<()> {
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
        private_dirs(&self.dir, &self.dir)?;
        let tmp = self.dir.join(format!("{PROGRESS_FILE}.tmp"));
        let mut file = private_options()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        file.write_all(&serde_json::to_vec(progress).map_err(io::Error::other)?)?;
        file.sync_all()?;
        fs::rename(&tmp, self.dir.join(PROGRESS_FILE))?;
        sync_dir(&self.dir)
    }

    fn load_progress(&self) -> io::Result<Progress> {
        match fs::read(self.dir.join(PROGRESS_FILE)) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Progress::default()),
            Err(e) => Err(e),
        }
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
    fn torn_last_line_is_read_without_error_and_not_glued() {
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
    fn saving_progress_makes_new_records_durable_first() {
        let dir = temp("sync");
        let mut sink = FileSink::new(&dir);
        sink.append(&turn("claude", "s1", "2026-11-11T10:00:00Z", "a"))
            .unwrap();
        assert_eq!(sink.unsynced.len(), 1);
        sink.save_progress(&Progress::default()).unwrap();
        assert!(sink.unsynced.is_empty());
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
        assert_eq!(file_name("a/b"), file_name("a/b"));
        assert_ne!(file_name("a/b"), file_name("a/c"));
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
