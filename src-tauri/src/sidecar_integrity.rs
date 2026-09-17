// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Integrity check for the sidecars Meteor runs elevated (`PresentMon.exe`,
//! `cputemp.exe`).
//!
//! Both are resources of a per-user install, so they live in a folder the user
//! (and anything running as the user) can write. Starting them from an elevated
//! Meteor without a check would run whatever was dropped there with admin
//! rights. `build.rs` embeds the SHA-256 of the exact files that were bundled;
//! a binary that does not match is never started.
//!
//! The file is hashed through a handle opened without write or delete sharing
//! and that handle is kept open until the process has been created, so the file
//! cannot be swapped between the check and `CreateProcess`.

use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

pub const PRESENTMON_SHA256: Option<&str> = option_env!("METEOR_PRESENTMON_SHA256");
pub const CPUTEMP_SHA256: Option<&str> = option_env!("METEOR_CPUTEMP_SHA256");

/// A verified binary. Keep it alive until the child process has been spawned.
#[must_use = "the file can be replaced as soon as this is dropped"]
pub struct Pinned(#[allow(dead_code)] File);

/// Open `path`, deny writers and deleters for as long as the result lives, and
/// check its SHA-256 against the build-time value. No build-time value (a build
/// without the binaries) fails closed.
pub fn open_verified(path: &Path, expected: Option<&str>) -> io::Result<Pinned> {
    let expected = expected.ok_or_else(|| {
        io::Error::other(format!(
            "{} has no build-time SHA-256; refusing to start it",
            path.display()
        ))
    })?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x1;
        options.share_mode(FILE_SHARE_READ);
    }
    let mut file = options.open(path)?;
    let actual = sha256_hex(&mut file)?;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} does not match the SHA-256 it was built with; refusing to start it",
                path.display()
            ),
        ));
    }
    Ok(Pinned(file))
}

fn sha256_hex(reader: &mut impl Read) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn temp_file(name: &str, contents: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("meteor-integrity-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn hashes_match_the_reference_vector() {
        assert_eq!(sha256_hex(&mut &b"abc"[..]).unwrap(), ABC_SHA256);
    }

    #[test]
    fn a_tampered_or_unhashed_binary_is_refused() {
        // Regression (BD2): elevated sidecars were started from a user-writable
        // folder with no check at all.
        let path = temp_file("tampered.bin", b"abd");
        assert!(open_verified(&path, Some(ABC_SHA256)).is_err());
        let path = temp_file("genuine.bin", b"abc");
        assert!(open_verified(&path, None).is_err());
        assert!(open_verified(&path, Some(&ABC_SHA256.to_uppercase())).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn the_bundled_sidecars_match_the_hashes_build_rs_embedded() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries");
        for (file, hash) in [("PresentMon.exe", PRESENTMON_SHA256), ("cputemp.exe", CPUTEMP_SHA256)] {
            assert!(open_verified(&dir.join(file), hash).is_ok(), "{file}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_pinned_binary_can_run_but_cannot_be_replaced() {
        let system = std::env::var_os("SystemRoot").expect("SystemRoot");
        let original = Path::new(&system).join("System32").join("where.exe");
        let bytes = std::fs::read(original).unwrap();
        let path = temp_file("where.exe", &bytes);
        let expected = sha256_hex(&mut &bytes[..]).unwrap();

        let pinned = open_verified(&path, Some(&expected)).unwrap();
        assert!(
            std::fs::OpenOptions::new().write(true).open(&path).is_err(),
            "writable while pinned"
        );
        assert!(std::fs::remove_file(&path).is_err(), "deletable while pinned");
        let status = std::process::Command::new(&path)
            .arg("/?")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        assert!(status.is_ok(), "CreateProcess refused a pinned image: {status:?}");
        drop(pinned);
        std::fs::remove_file(&path).unwrap();
    }
}
