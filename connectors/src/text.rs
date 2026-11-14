use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The sessions directory of a tool: `<override>/<sub>` when the environment variable
/// holds a non-empty value, else `<home>/<default_dir>/<sub>`.
pub fn root_under(
    home: &Path,
    env_value: Option<OsString>,
    default_dir: &str,
    sub: &str,
) -> PathBuf {
    match env_value.filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir).join(sub),
        None => home.join(default_dir).join(sub),
    }
}

/// The non-empty texts one after the other; `None` when none is left.
pub fn join_texts<'a>(texts: impl Iterator<Item = &'a str>) -> Option<String> {
    let texts: Vec<&str> = texts.filter(|t| !t.is_empty()).collect();
    (!texts.is_empty()).then(|| texts.join("\n\n"))
}

/// Whether `text`, after leading whitespace, starts with one of the `markers` a tool puts
/// into messages the user did not type.
pub fn starts_with_marker(text: &str, markers: &[&str]) -> bool {
    let text = text.trim_start();
    markers.iter().any(|m| text.starts_with(m))
}

/// The entries of `dir`; a missing directory has none.
pub fn entries_of(dir: &Path) -> io::Result<Vec<PathBuf>> {
    match fs::read_dir(dir) {
        Ok(entries) => entries.map(|e| e.map(|e| e.path())).collect(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Whether `path` is a file whose name ends in `.jsonl`.
pub fn is_jsonl_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "jsonl") && path.is_file()
}

/// A directory for a test, removed when it goes out of scope.
#[cfg(test)]
pub struct Scratch(PathBuf);

#[cfg(test)]
impl Scratch {
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("connectors-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Writes `text` to `rel` under the directory, creating the folders on the way.
    pub fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }
}

#[cfg(test)]
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
