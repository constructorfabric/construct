//! pgvector, built by `build.rs` against the bundled PostgreSQL and embedded in the agent.
//! It is copied into the PostgreSQL installation on every start, so a new agent
//! brings its own pgvector and nothing is downloaded.

use std::fs;
use std::io;
use std::path::Path;

/// The file name PostgreSQL loads the library by (its `DLSUFFIX`); `build.rs` writes the same name.
#[cfg(target_os = "macos")]
const LIBRARY_NAME: &str = "vector.dylib";
#[cfg(not(target_os = "macos"))]
const LIBRARY_NAME: &str = "vector.so";
#[cfg(target_os = "macos")]
const LIBRARY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pgvector/vector.dylib"));
#[cfg(not(target_os = "macos"))]
const LIBRARY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pgvector/vector.so"));
const CONTROL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pgvector/vector.control"));
const SQL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pgvector/vector.sql"));
const PG_VERSION: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pgvector/vector.pgversion"));

/// The pgvector version the agent carries, from its control file.
///
/// # Errors
/// Fails when this platform has no pgvector build.
pub fn version() -> io::Result<String> {
    if LIBRARY.is_empty() {
        return Err(unsupported());
    }
    default_version(CONTROL)
}

fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "pgvector is not built for this platform yet",
    )
}

/// Copy the extension files into the PostgreSQL installation `install_dir`
/// (`<install_dir>/lib` and `<install_dir>/share/extension`), which must be the release
/// `pg_version` that pgvector was built against. The SQL file is named after the version in
/// the control file, which is what `CREATE EXTENSION` looks for.
///
/// # Errors
/// Fails when this platform has no pgvector build, `pg_version` is not the release it was
/// built for, or a file cannot be written.
pub fn install(install_dir: &Path, pg_version: &str) -> io::Result<()> {
    let version = version()?;
    let built_for = String::from_utf8_lossy(PG_VERSION);
    if built_for != pg_version {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("pgvector was built for PostgreSQL {built_for}, not {pg_version}"),
        ));
    }
    let extension_dir = install_dir.join("share").join("extension");
    fs::create_dir_all(&extension_dir)?;
    fs::create_dir_all(install_dir.join("lib"))?;
    write_if_changed(&install_dir.join("lib").join(LIBRARY_NAME), LIBRARY)?;
    write_if_changed(&extension_dir.join("vector.control"), CONTROL)?;
    write_if_changed(&extension_dir.join(format!("vector--{version}.sql")), SQL)
}

/// The `default_version = '...'` line of a control file.
fn default_version(control: &[u8]) -> io::Result<String> {
    String::from_utf8_lossy(control)
        .lines()
        .find_map(|line| {
            let value = line.strip_prefix("default_version")?;
            let value = value.trim_start().strip_prefix('=')?;
            Some(value.trim().trim_matches('\'').to_string())
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no default_version in control"))
}

/// Write `bytes` unless the file already has them; replaced atomically so a running
/// PostgreSQL never maps a half-written library.
fn write_if_changed(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(());
    }
    let tmp = path.with_extension("new");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("cb-pgv-{name}-{}", std::process::id()))
    }

    #[test]
    fn reads_the_default_version() {
        let control =
            b"comment = 'x'\ndefault_version = '0.8.7'\nmodule_pathname = '$libdir/vector'\n";
        assert_eq!(default_version(control).unwrap(), "0.8.7");
    }

    #[test]
    fn control_without_a_version_is_an_error() {
        assert!(default_version(b"comment = 'x'\n").is_err());
    }

    #[test]
    fn install_writes_the_embedded_files_and_is_repeatable() {
        if LIBRARY.is_empty() {
            assert_eq!(
                install(&scratch("none"), "16.0.0").unwrap_err().kind(),
                io::ErrorKind::Unsupported
            );
            return;
        }
        let dir = scratch("ok");
        let pg = String::from_utf8_lossy(PG_VERSION).into_owned();
        install(&dir, &pg).unwrap();
        install(&dir, &pg).unwrap();
        let version = version().unwrap();
        assert_eq!(
            fs::read(dir.join("lib").join(LIBRARY_NAME)).unwrap(),
            LIBRARY
        );
        assert_eq!(
            fs::read(dir.join("share/extension/vector.control")).unwrap(),
            CONTROL
        );
        assert_eq!(
            fs::read(dir.join(format!("share/extension/vector--{version}.sql"))).unwrap(),
            SQL
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn install_refuses_another_postgresql_release() {
        if LIBRARY.is_empty() {
            return;
        }
        let dir = scratch("other");
        let err = install(&dir, "15.0.0").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(!dir.exists());
    }
}
