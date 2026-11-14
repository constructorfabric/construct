use std::ffi::OsString;
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
