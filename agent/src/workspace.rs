//! The workspace: one folder, `workspace`, in the data directory, where the agent reads and
//! writes the user's text files.
//!
//! The folder is opened once with `cap-std` and the agent keeps the open `Dir`. Every access is
//! relative to that handle, so the operating system itself refuses a path that leads out of the
//! folder (an escaping `..`, an absolute path, a link that points outside). The module never
//! builds a path string from the folder's absolute path and never compares paths as strings to
//! decide what is inside.
//!
//! Only regular files with UTF-8 text are handled. There is no limit on file size, file count or
//! depth. The calls block; the caller runs them on a thread of its own.
//!
//! Errors are a plain enum with fixed texts. They never hold a path or any file content.

use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use cap_std::ambient_authority;
use cap_std::fs::{
    Dir, DirBuilder, DirBuilderExt, OpenOptions, OpenOptionsExt, Permissions, PermissionsExt,
};
use cap_tempfile::TempFile;
use globset::GlobBuilder;

use crate::ipc::create_private_dir;

/// Name of the folder inside the data directory.
pub const DIR_NAME: &str = "workspace";

/// Mode of the folders the workspace makes: the user only.
const DIR_MODE: u32 = 0o700;
/// Mode of the files the workspace makes: the user only.
const FILE_MODE: u32 = 0o600;

/// Why a workspace call failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The path is empty, absolute, leads out of the workspace or holds a NUL byte.
    InvalidPath,
    /// The file or folder does not exist.
    NotFound,
    /// The path is not a regular file.
    NotRegularFile,
    /// The file is not UTF-8 text.
    NotText,
    /// The text to replace is empty.
    EmptyOld,
    /// The text to replace is the same as the new text.
    Unchanged,
    /// The text to replace is not in the file.
    NoMatch,
    /// The file changed between reading it and replacing it, so the edit was not made.
    Changed,
    /// The text to replace occurs more than once and `replace_all` is not set.
    NotUnique,
    /// The glob pattern is not valid.
    InvalidPattern,
    /// Any other failure of the file system; the kind of it, never a path.
    Io(io::ErrorKind),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath => f.write_str("the path is not inside the workspace"),
            Self::NotFound => f.write_str("no such file or folder"),
            Self::NotRegularFile => f.write_str("not a regular file"),
            Self::NotText => f.write_str("the file is not UTF-8 text"),
            Self::EmptyOld => f.write_str("the text to replace is empty"),
            Self::Unchanged => f.write_str("the old and the new text are the same"),
            Self::NoMatch => f.write_str("the text to replace was not found"),
            Self::Changed => f.write_str("the file changed while it was edited"),
            Self::NotUnique => f.write_str("the text to replace occurs more than once"),
            Self::InvalidPattern => f.write_str("the glob pattern is not valid"),
            Self::Io(kind) => write!(f, "file system error: {kind}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::NotFound,
            // The folder was left out of reach by a link: the same refusal as a bad path.
            io::ErrorKind::PermissionDenied | io::ErrorKind::InvalidInput => Self::InvalidPath,
            kind => Self::Io(kind),
        }
    }
}

/// The open workspace folder.
#[derive(Debug)]
pub struct Workspace {
    root: Dir,
}

impl Workspace {
    /// Creates `workspace` in `data_dir` (and `data_dir` itself when it is missing), makes the
    /// folder user-only (mode `0700`, also when it already exists with another mode) and opens it.
    ///
    /// # Errors
    /// The folder cannot be created, its mode cannot be set or it cannot be opened.
    pub fn open(data_dir: &Path) -> Result<Self, Error> {
        let path = data_dir.join(DIR_NAME);
        create_private_dir(&path)?;
        Ok(Self {
            root: Dir::open_ambient_dir(&path, ambient_authority())?,
        })
    }

    /// Returns the lines of a file. `offset` is the number of the first line to return (the first
    /// line of the file is 1, so 0 means the same); `limit` is the largest number of lines.
    ///
    /// # Errors
    /// The path is not valid, the file is missing, not a regular file or not UTF-8 text.
    pub fn read(
        &self,
        path: &str,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<Vec<String>, Error> {
        let text = self.read_text(&relative(path)?)?;
        let skip = offset.map_or(0, |n| n.saturating_sub(1));
        Ok(text
            .lines()
            .skip(skip)
            .take(limit.unwrap_or(usize::MAX))
            .map(str::to_owned)
            .collect())
    }

    /// Replaces the file, or creates it with the folders of its path, in one step: the content
    /// goes to a temporary file in the same folder, which is renamed over the old file.
    ///
    /// # Errors
    /// The path is not valid, the content is not text or the target exists and is not a regular
    /// file.
    pub fn write(&self, path: &str, content: &str) -> Result<(), Error> {
        let path = relative(path)?;
        if content.contains('\0') {
            return Err(Error::NotText);
        }
        let (parent, name) = split(&path)?;
        match self.root.metadata(&path) {
            Ok(meta) if !meta.is_file() => return Err(Error::NotRegularFile),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if !parent.as_os_str().is_empty() {
            let mut builder = DirBuilder::new();
            builder.recursive(true).mode(DIR_MODE);
            self.root.create_dir_with(parent, &builder)?;
        }
        let dir = self.dir(parent)?;
        let mut file = TempFile::new(&dir)?;
        file.as_file_mut()
            .set_permissions(Permissions::from_mode(FILE_MODE))?;
        file.as_file_mut().write_all(content.as_bytes())?;
        file.replace(name)?;
        Ok(())
    }

    /// Replaces `old` with `new` in a file: the one place it occurs, or every place when
    /// `replace_all` is set.
    ///
    /// # Errors
    /// The file cannot be read, `old` is empty, equals `new`, is not found, or occurs more than
    /// once while `replace_all` is not set.
    pub fn edit(&self, path: &str, old: &str, new: &str, replace_all: bool) -> Result<(), Error> {
        self.edit_with(path, old, new, replace_all, || {})
    }

    /// `edit`, with `between` run after the file is read and before it is checked again.
    fn edit_with(
        &self,
        path: &str,
        old: &str,
        new: &str,
        replace_all: bool,
        between: impl FnOnce(),
    ) -> Result<(), Error> {
        let path = relative(path)?;
        if old.is_empty() {
            return Err(Error::EmptyOld);
        }
        if old == new {
            return Err(Error::Unchanged);
        }
        if new.contains('\0') {
            return Err(Error::NotText);
        }
        let text = self.read_text(&path)?;
        let updated = match text.matches(old).count() {
            0 => return Err(Error::NoMatch),
            1 => text.replacen(old, new, 1),
            _ if replace_all => text.replace(old, new),
            _ => return Err(Error::NotUnique),
        };
        between();
        // The file is read again just before it is replaced: an edit made on a text that is no
        // longer in the file would throw the other change away. A change in the short time between
        // this read and the replacement cannot be seen without a lock on the file.
        if self.read_text(&path)? != text {
            return Err(Error::Changed);
        }
        self.write(&path.to_string_lossy(), &updated)
    }

    /// Returns the regular files below `path` (the workspace itself when it is empty) whose path
    /// relative to `path` matches the glob `pattern`, for example `**/*.json`. The result is
    /// sorted and holds paths relative to the workspace. Links are not followed.
    ///
    /// # Errors
    /// The pattern or the path is not valid, or the folder is missing.
    pub fn glob(&self, pattern: &str, path: &str) -> Result<Vec<String>, Error> {
        let matcher = GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .map_err(|_| Error::InvalidPattern)?
            .compile_matcher();
        let base = if path.is_empty() {
            PathBuf::new()
        } else {
            relative(path)?
        };
        let start = self.dir(&base)?;

        let mut found = Vec::new();
        // Only paths are queued and a folder is opened when it is visited, so the number of open
        // folders does not grow with the number of folders in the workspace.
        let mut pending = vec![PathBuf::new()];
        while let Some(prefix) = pending.pop() {
            let dir = if prefix.as_os_str().is_empty() {
                start.try_clone()?
            } else {
                start.open_dir(&prefix)?
            };
            for entry in dir.entries()? {
                let entry = entry?;
                let kind = entry.file_type()?;
                let name = prefix.join(entry.file_name());
                if kind.is_dir() {
                    pending.push(name);
                } else if kind.is_file()
                    && matcher.is_match(&name)
                    && let Some(name) = base.join(&name).to_str()
                {
                    found.push(name.to_owned());
                }
            }
        }
        found.sort();
        Ok(found)
    }

    fn read_text(&self, path: &Path) -> Result<String, Error> {
        // Not blocking: a named pipe with no writer is opened at once and refused below instead of
        // holding the call for ever.
        let mut options = OpenOptions::new();
        options.read(true).custom_flags(libc::O_NONBLOCK);
        let mut file = self.root.open_with(path, &options)?;
        if !file.metadata()?.is_file() {
            return Err(Error::NotRegularFile);
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        if bytes.contains(&0) {
            return Err(Error::NotText);
        }
        String::from_utf8(bytes).map_err(|_| Error::NotText)
    }

    /// The folder at `path`, or the workspace itself for an empty path.
    fn dir(&self, path: &Path) -> Result<Dir, Error> {
        if path.as_os_str().is_empty() {
            Ok(self.root.try_clone()?)
        } else {
            Ok(self.root.open_dir(path)?)
        }
    }
}

/// Checks a path given by the caller and returns it without `.` parts and without the `..` parts
/// that stay inside. A path that is empty, absolute, holds a NUL byte or whose `..` would climb
/// above the workspace is refused. This is a first refusal with a clear error; the confinement
/// itself is the open `Dir`.
fn relative(path: &str) -> Result<PathBuf, Error> {
    if path.contains('\0') {
        return Err(Error::InvalidPath);
    }
    let mut clean = PathBuf::new();
    let mut depth = 0usize;
    for part in Path::new(path).components() {
        match part {
            Component::Normal(name) => {
                clean.push(name);
                depth += 1;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if depth == 0 {
                    return Err(Error::InvalidPath);
                }
                clean.pop();
                depth -= 1;
            }
            Component::RootDir | Component::Prefix(_) => return Err(Error::InvalidPath),
        }
    }
    if depth == 0 {
        return Err(Error::InvalidPath);
    }
    Ok(clean)
}

/// The folder part and the file name of a checked path.
fn split(path: &Path) -> Result<(&Path, &std::ffi::OsStr), Error> {
    let name = path.file_name().ok_or(Error::InvalidPath)?;
    Ok((path.parent().unwrap_or(Path::new("")), name))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn open() -> (tempfile_dir::Dir, Workspace) {
        let data = tempfile_dir::Dir::new();
        let workspace = Workspace::open(data.path()).unwrap();
        (data, workspace)
    }

    /// A data directory that removes itself, without a new dependency.
    mod tempfile_dir {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicU32, Ordering};

        static NEXT: AtomicU32 = AtomicU32::new(0);

        pub struct Dir(PathBuf);

        impl Dir {
            pub fn new() -> Self {
                let path = std::env::temp_dir().join(format!(
                    "construct-workspace-test-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                std::fs::create_dir_all(&path).unwrap();
                Self(path)
            }

            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn open_creates_the_folder_with_mode_0700() {
        let (data, _workspace) = open();
        let meta = fs::metadata(data.path().join(DIR_NAME)).unwrap();
        assert!(meta.is_dir());
        assert_eq!(meta.permissions().mode() & 0o777, 0o700);
    }

    #[test]
    fn open_makes_an_existing_folder_user_only_and_creates_a_missing_data_dir() {
        let (data, _workspace) = open();
        let folder = data.path().join(DIR_NAME);
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
        Workspace::open(data.path()).unwrap();
        let meta = fs::metadata(&folder).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o700);

        let nested = data.path().join("new/data");
        Workspace::open(&nested).unwrap();
        assert!(nested.join(DIR_NAME).is_dir());
    }

    #[test]
    fn open_keeps_an_existing_folder() {
        let (data, workspace) = open();
        workspace.write("a.txt", "kept").unwrap();
        let again = Workspace::open(data.path()).unwrap();
        assert_eq!(again.read("a.txt", None, None).unwrap(), ["kept"]);
    }

    #[test]
    fn read_with_and_without_offset_and_limit() {
        let (_data, ws) = open();
        ws.write("t.txt", "one\ntwo\nthree\nfour\n").unwrap();
        assert_eq!(
            ws.read("t.txt", None, None).unwrap(),
            ["one", "two", "three", "four"]
        );
        assert_eq!(
            ws.read("t.txt", Some(2), None).unwrap(),
            ["two", "three", "four"]
        );
        assert_eq!(ws.read("t.txt", None, Some(2)).unwrap(), ["one", "two"]);
        assert_eq!(
            ws.read("t.txt", Some(2), Some(2)).unwrap(),
            ["two", "three"]
        );
        assert_eq!(ws.read("t.txt", Some(0), Some(1)).unwrap(), ["one"]);
        assert!(ws.read("t.txt", Some(9), None).unwrap().is_empty());
    }

    #[test]
    fn read_of_a_missing_file_or_a_folder_fails() {
        let (_data, ws) = open();
        ws.write("d/f.txt", "x").unwrap();
        assert_eq!(ws.read("none.txt", None, None), Err(Error::NotFound));
        assert_eq!(ws.read("d", None, None), Err(Error::NotRegularFile));
    }

    #[test]
    fn write_makes_the_folders_and_replaces_the_file() {
        let (data, ws) = open();
        ws.write("a/b/c.txt", "first").unwrap();
        ws.write("a/b/c.txt", "second").unwrap();
        let on_disk = fs::read_to_string(data.path().join("workspace/a/b/c.txt")).unwrap();
        assert_eq!(on_disk, "second");
        let names: Vec<_> = fs::read_dir(data.path().join("workspace/a/b"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["c.txt"], "no temporary file is left behind");
    }

    #[test]
    fn write_refuses_a_folder_as_target() {
        let (_data, ws) = open();
        ws.write("d/f.txt", "x").unwrap();
        assert_eq!(ws.write("d", "x"), Err(Error::NotRegularFile));
    }

    #[test]
    fn edit_replaces_one_occurrence() {
        let (_data, ws) = open();
        ws.write("e.txt", "alpha beta").unwrap();
        ws.edit("e.txt", "beta", "gamma", false).unwrap();
        assert_eq!(ws.read("e.txt", None, None).unwrap(), ["alpha gamma"]);
    }

    #[test]
    fn edit_errors() {
        let (_data, ws) = open();
        ws.write("e.txt", "x x").unwrap();
        assert_eq!(ws.edit("e.txt", "y", "z", false), Err(Error::NoMatch));
        assert_eq!(ws.edit("e.txt", "x", "z", false), Err(Error::NotUnique));
        assert_eq!(ws.edit("e.txt", "x", "x", false), Err(Error::Unchanged));
        assert_eq!(ws.edit("e.txt", "", "x", false), Err(Error::EmptyOld));
        assert_eq!(ws.read("e.txt", None, None).unwrap(), ["x x"]);
    }

    #[test]
    fn edit_notices_a_change_between_the_read_and_the_replacement() {
        let (data, ws) = open();
        ws.write("e.txt", "alpha beta").unwrap();
        let file = data.path().join("workspace/e.txt");
        let result = ws.edit_with("e.txt", "beta", "gamma", false, || {
            fs::write(&file, "alpha beta, changed by another program").unwrap();
        });
        assert_eq!(result, Err(Error::Changed));
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "alpha beta, changed by another program"
        );

        // The file is removed, or replaced by a folder, in that time.
        let result = ws.edit_with("e.txt", "beta", "gamma", false, || {
            fs::remove_file(&file).unwrap();
        });
        assert_eq!(result, Err(Error::NotFound));
        assert!(!file.exists());
    }

    #[test]
    fn edit_replace_all() {
        let (_data, ws) = open();
        ws.write("e.txt", "x x x").unwrap();
        ws.edit("e.txt", "x", "yy", true).unwrap();
        assert_eq!(ws.read("e.txt", None, None).unwrap(), ["yy yy yy"]);
    }

    #[test]
    fn glob_returns_sorted_relative_files_only() {
        let (_data, ws) = open();
        ws.write("b.json", "{}").unwrap();
        ws.write("a/z.json", "{}").unwrap();
        ws.write("a/deep/y.json", "{}").unwrap();
        ws.write("a/note.txt", "n").unwrap();
        ws.write("a/folder.json/inner.txt", "n").unwrap();
        assert_eq!(
            ws.glob("**/*.json", "").unwrap(),
            ["a/deep/y.json", "a/z.json", "b.json"]
        );
        assert_eq!(ws.glob("*.json", "").unwrap(), ["b.json"]);
        assert_eq!(
            ws.glob("**/*.json", "a").unwrap(),
            ["a/deep/y.json", "a/z.json"]
        );
        assert_eq!(ws.glob("*.json", "a").unwrap(), ["a/z.json"]);
        assert!(ws.glob("*.md", "").unwrap().is_empty());
    }

    #[test]
    fn glob_handles_more_folders_than_open_files_allowed() {
        let (_data, ws) = open();
        for i in 0..3000 {
            ws.write(&format!("d{i}/f.json"), "{}").unwrap();
        }
        assert_eq!(ws.glob("**/*.json", "").unwrap().len(), 3000);
    }

    #[test]
    fn glob_refuses_a_bad_pattern_and_a_bad_path() {
        let (_data, ws) = open();
        assert_eq!(ws.glob("[", ""), Err(Error::InvalidPattern));
        assert_eq!(ws.glob("*", "../x"), Err(Error::InvalidPath));
        assert_eq!(ws.glob("*", "missing"), Err(Error::NotFound));
    }

    #[test]
    fn binary_data_and_invalid_utf8_are_refused() {
        let (data, ws) = open();
        let root = data.path().join(DIR_NAME);
        fs::write(root.join("nul.bin"), b"ab\0cd").unwrap();
        fs::write(root.join("bad.txt"), [0xff, 0xfe, b'a']).unwrap();
        assert_eq!(ws.read("nul.bin", None, None), Err(Error::NotText));
        assert_eq!(ws.read("bad.txt", None, None), Err(Error::NotText));
        assert_eq!(ws.edit("bad.txt", "a", "b", false), Err(Error::NotText));
        assert_eq!(ws.write("w.txt", "a\0b"), Err(Error::NotText));
        assert_eq!(ws.edit("nul.bin", "a", "b", false), Err(Error::NotText));
        ws.write("ok.txt", "a").unwrap();
        assert_eq!(ws.edit("ok.txt", "a", "b\0", false), Err(Error::NotText));
        assert_eq!(ws.read("ok.txt", None, None).unwrap(), ["a"]);
    }

    #[test]
    fn escaping_absolute_and_nul_paths_are_refused() {
        let (data, ws) = open();
        fs::write(data.path().join("outside.txt"), "secret").unwrap();
        for path in [
            "../outside.txt",
            "a/../../outside.txt",
            "/etc/passwd",
            "",
            ".",
            "a\0b",
        ] {
            assert_eq!(
                ws.read(path, None, None),
                Err(Error::InvalidPath),
                "{path:?}"
            );
            assert_eq!(ws.write(path, "x"), Err(Error::InvalidPath), "{path:?}");
            assert_eq!(
                ws.edit(path, "a", "b", false),
                Err(Error::InvalidPath),
                "{path:?}"
            );
            if !path.is_empty() && path != "." {
                assert_eq!(ws.glob("*", path), Err(Error::InvalidPath), "{path:?}");
            }
        }
        assert_eq!(
            fs::read_to_string(data.path().join("outside.txt")).unwrap(),
            "secret"
        );
    }

    #[test]
    fn a_dotdot_that_stays_inside_is_allowed() {
        let (_data, ws) = open();
        ws.write("a/b.txt", "in").unwrap();
        assert_eq!(ws.read("a/../a/./b.txt", None, None).unwrap(), ["in"]);
    }

    #[test]
    fn no_error_text_holds_an_absolute_path() {
        let (data, ws) = open();
        let absolute = data.path().to_string_lossy().into_owned();
        ws.write("d/f.txt", "x").unwrap();
        fs::write(data.path().join("ok.txt"), "x").unwrap();
        let errors = [
            ws.read("none.txt", None, None).unwrap_err(),
            ws.read("d", None, None).unwrap_err(),
            ws.read("../x", None, None).unwrap_err(),
            ws.write("d", "x").unwrap_err(),
            ws.edit("d/f.txt", "q", "r", false).unwrap_err(),
            ws.glob("*", "missing").unwrap_err(),
            Workspace::open(&data.path().join("ok.txt")).unwrap_err(),
        ];
        for error in errors {
            let text = format!("{error} {error:?}");
            assert!(!text.contains(&absolute), "{text}");
            assert!(!text.contains('/'), "{text}");
        }
    }
}
