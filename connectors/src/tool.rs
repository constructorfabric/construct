use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Record;

/// What a reader remembers about one file between lines. It can be saved and restored,
/// so reading continues after a restart in the middle of a file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileState {
    pub session: String,
    pub folder: String,
    /// Empty when unknown.
    pub branch: String,
}

/// A coding tool whose session files are read.
pub trait Tool: Send + Sync {
    /// Short stable name; it is the `tool` of every record and a directory name on disk.
    fn name(&self) -> &'static str;

    /// Where the tool keeps its session files, for a user's home directory.
    fn root(&self, home: &Path) -> PathBuf;

    /// The session files under `root`.
    ///
    /// # Errors
    /// Fails when the directory cannot be read; a missing `root` is not an error.
    fn find_files(&self, root: &Path) -> io::Result<Vec<PathBuf>>;

    /// Reads one line of a file. `state` carries the session id, folder and branch learned
    /// from earlier lines of the same file. Lines of no interest give `None`.
    fn read_line(&self, state: &mut FileState, line: &str) -> Option<Record>;
}

/// A list of tools.
pub struct Registry(Vec<Box<dyn Tool>>);

impl Registry {
    #[must_use]
    pub fn new(tools: Vec<Box<dyn Tool>>) -> Self {
        Self(tools)
    }

    /// The tools this crate knows.
    #[must_use]
    pub fn builtin() -> Self {
        Self(tools())
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn Tool> {
        self.0.iter().map(AsRef::as_ref)
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.iter().find(|t| t.name() == name)
    }
}

/// The tools of this crate. A new tool is one module and one line here.
#[must_use]
pub fn tools() -> Vec<Box<dyn Tool>> {
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Role, Turn};

    /// A tool defined outside the crate's modules: its own type and one line in a list.
    struct Dummy;

    impl Tool for Dummy {
        fn name(&self) -> &'static str {
            "dummy"
        }

        fn root(&self, home: &Path) -> PathBuf {
            home.join(".dummy")
        }

        fn find_files(&self, root: &Path) -> io::Result<Vec<PathBuf>> {
            Ok(vec![root.join("s.log")])
        }

        fn read_line(&self, state: &mut FileState, line: &str) -> Option<Record> {
            if let Some(id) = line.strip_prefix("session ") {
                state.session = id.to_owned();
                return None;
            }
            Some(Record::Turn(Turn {
                tool: self.name().into(),
                session: state.session.clone(),
                time: "2026-11-11T10:00:00Z".into(),
                folder: state.folder.clone(),
                branch: state.branch.clone(),
                role: Role::User,
                text: line.to_owned(),
            }))
        }
    }

    #[test]
    fn a_new_tool_is_one_type_and_one_line() {
        let registry = Registry::new(vec![Box::new(Dummy)]);
        let tool = registry.get("dummy").unwrap();
        assert_eq!(tool.root(Path::new("/h")), Path::new("/h/.dummy"));
        assert_eq!(tool.find_files(Path::new("/r")).unwrap().len(), 1);
        let mut state = FileState::default();
        assert!(tool.read_line(&mut state, "session abc").is_none());
        let saved = serde_json::to_string(&state).unwrap();
        let mut restored: FileState = serde_json::from_str(&saved).unwrap();
        let record = tool.read_line(&mut restored, "hello").unwrap();
        assert_eq!(record.session(), "abc");
        assert!(registry.get("other").is_none());
    }
}
