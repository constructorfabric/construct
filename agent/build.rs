//! Builds pgvector for the bundled PostgreSQL (see `scripts/build-pgvector.sh`) and leaves
//! `vector.so`, `vector.control`, `vector.sql` and `vector.pgversion` in `OUT_DIR` for `src/pgvector.rs`
//! to embed. On platforms other than Linux `x86_64` the files are empty and the agent
//! reports that there is no pgvector there.

use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<(), Box<dyn Error>> {
    let out = PathBuf::from(std::env::var("OUT_DIR")?).join("pgvector");
    let script = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?)
        .join("..")
        .join("scripts")
        .join("build-pgvector.sh");
    println!("cargo:rerun-if-changed={}", script.display());
    println!("cargo:rerun-if-env-changed=POSTGRESQL_VERSION");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS")?;
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH")?;
    if target_os == "linux" && target_arch == "x86_64" {
        // Cargo's build variables (`DEBUG`, `OPT_LEVEL`, ...) would reach `make` and change
        // the compiler flags, so the script gets a clean environment.
        let mut command = Command::new("bash");
        command.arg(&script).arg(&out).env_clear();
        for var in ["PATH", "HOME", "TMPDIR", "POSTGRESQL_VERSION"] {
            if let Some(value) = std::env::var_os(var) {
                command.env(var, value);
            }
        }
        let status = command.status()?;
        if !status.success() {
            return Err(format!("{} failed: {status}", script.display()).into());
        }
    } else {
        std::fs::create_dir_all(&out)?;
        for name in [
            "vector.so",
            "vector.control",
            "vector.sql",
            "vector.pgversion",
        ] {
            std::fs::write(out.join(name), b"")?;
        }
    }
    Ok(())
}
