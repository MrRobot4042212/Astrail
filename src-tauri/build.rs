/// Build-time secrets that `src/igdb.rs` reads with `option_env!`.
const ENV_KEYS: &[&str] = &["IGDB_CLIENT_ID", "IGDB_CLIENT_SECRET"];

/// Load `IGDB_CLIENT_ID` / `IGDB_CLIENT_SECRET` from the repo-root `.env` for
/// local builds.
///
/// `option_env!` only sees what is in rustc's environment, and nothing in the
/// tool chain (`tauri dev`, `cargo`, `next`) exports `.env` to cargo, so until
/// now "copy `.env.example` to `.env`" produced a build with cover lookups
/// disabled and no error. A variable already present in the environment (CI
/// injects the repository secrets) always wins over the file, so CI cannot be
/// hijacked by a stray `.env` and a developer can still override per shell.
/// Values are forwarded with `cargo:rustc-env`, which is exactly what
/// `option_env!` reads; they never appear in build output.
fn load_dotenv() {
    use std::path::Path;

    for key in ENV_KEYS {
        // Re-run when the shell value changes, not only when the file does.
        println!("cargo:rerun-if-env-changed={key}");
    }

    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.env");
    // Only declared when the file exists: cargo treats a missing
    // `rerun-if-changed` path as "always dirty" on some versions, which would
    // recompile the crate on every build for everyone without a `.env`.
    // Consequence: creating `.env` later needs a `cargo clean -p meteor` (or
    // any other change to this script's inputs) to be picked up.
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return;
    };
    println!("cargo:rerun-if-changed={}", path.display());

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if !ENV_KEYS.contains(&key) || std::env::var_os(key).is_some() {
            continue;
        }
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if value.is_empty() {
            continue;
        }
        println!("cargo:rustc-env={key}={value}");
    }
}

/// Embed the SHA-256 of the sidecars that get bundled, so the app refuses to
/// start a different binary elevated (see `src/sidecar_integrity.rs`).
#[cfg(windows)]
fn embed_sidecar_hashes() {
    use sha2::{Digest, Sha256};

    for (file, key) in [
        ("PresentMon.exe", "METEOR_PRESENTMON_SHA256"),
        ("cputemp.exe", "METEOR_CPUTEMP_SHA256"),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join(file);
        // A missing binary already fails the build in tauri-build (it is a
        // declared resource); without a hash the app refuses to start it.
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        println!("cargo:rerun-if-changed={}", path.display());
        let hex: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        println!("cargo:rustc-env={key}={hex}");
    }
}

fn main() {
    load_dotenv();
    #[cfg(windows)]
    embed_sidecar_hashes();

    tauri_build::build()
}
