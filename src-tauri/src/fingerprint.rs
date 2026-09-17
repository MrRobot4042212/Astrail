// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Cheap "did the installed-games set change?" probe.
//!
//! A full `get_library` spawns eight scanners, walks three registry hives and
//! (for Xbox) starts a PowerShell process. The frontend ran that every 15
//! minutes regardless of whether anything had changed — usually while the window
//! was hidden in the tray.
//!
//! This computes a fingerprint from the *containers* stores use — library
//! folders, manifest directories, uninstall registry keys — all of which change
//! their last-write time when a game is installed, moved or removed. Comparing
//! it costs a handful of `stat`/`RegQueryInfoKey` calls, so the frontend can
//! skip the expensive scan when the answer is "nothing new".
//!
//! Conservative by construction: anything it cannot read counts as *changed*, so
//! a false "no change" is not possible for a source it can see at all.

use std::path::Path;
use std::time::UNIX_EPOCH;

/// Drive roots (`C:\`, `D:\`…) that are actually fixed disks.
///
/// This used to probe all 26 letters with `Path::exists()`. On a machine with a
/// mapped-but-disconnected network drive that blocks on the SMB redirector
/// timeout — seconds, sometimes tens of seconds — inside a call the frontend
/// makes every 15 minutes; it also touched optical and removable drives for no
/// reason. `GetLogicalDrives` + `GetDriveTypeW` answers from a bitmask instead.
#[cfg(windows)]
fn fixed_drives() -> Vec<String> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    use windows::Win32::System::WindowsProgramming::DRIVE_FIXED;

    // SAFETY: no arguments, no pointers; returns a bitmask of present drives.
    let mask = unsafe { GetLogicalDrives() };
    (0u32..26)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| format!("{}:\\", (b'A' + i as u8) as char))
        .filter(|root| {
            let wide = HSTRING::from(root.as_str());
            // SAFETY: `wide` is a NUL-terminated wide string that outlives the call.
            unsafe { GetDriveTypeW(&wide) == DRIVE_FIXED }
        })
        .collect()
}

/// Mix a value into a running FNV-1a hash.
fn mix(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= *byte as u64;
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// Fold a path's last-write time and size into the hash. A missing path folds in
/// a marker instead, so "the folder disappeared" is itself a change.
fn mix_path(hash: &mut u64, path: &Path) {
    match std::fs::metadata(path) {
        Ok(md) => {
            let secs = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            mix(hash, &secs.to_le_bytes());
            mix(hash, &md.len().to_le_bytes());
        }
        Err(_) => mix(hash, b"missing"),
    }
}

/// Fingerprint of everything that would change the library contents.
pub fn compute() -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;

    // Steam: the library-folders manifest plus each library's steamapps dir
    // (whose mtime moves when an appmanifest is added or removed).
    if let Ok(steam) = steamlocate::SteamDir::locate() {
        mix_path(&mut hash, &steam.path().join("steamapps/libraryfolders.vdf"));
        if let Ok(libraries) = steam.libraries() {
            for library in libraries.flatten() {
                mix_path(&mut hash, &library.path().join("steamapps"));
            }
        }
    }

    // Epic: the manifests directory.
    if let Ok(program_data) = std::env::var("PROGRAMDATA") {
        mix_path(
            &mut hash,
            &Path::new(&program_data).join("Epic/EpicGamesLauncher/Data/Manifests"),
        );
    }

    // Xbox / Game Pass: the per-drive game roots, plus the AppX install root that
    // `xbox::scan_appx` actually reads. Without `WindowsApps` a Game Pass title
    // installed there moved nothing here, so `library_changed` answered "no" and
    // the cached AppX enumeration kept serving a stale list.
    #[cfg(windows)]
    for drive in fixed_drives() {
        mix_path(&mut hash, &Path::new(&drive).join("XboxGames"));
        mix_path(&mut hash, &Path::new(&drive).join("Program Files/WindowsApps"));
    }

    // Everything else (GOG, EA, Ubisoft, Battle.net, generic apps) is discovered
    // through the registry: fold in each hive's last-write time.
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        const KEYS: &[(isize, &str)] = &[
            (
                HKEY_LOCAL_MACHINE,
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            ),
            (
                HKEY_LOCAL_MACHINE,
                r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
            ),
            (
                HKEY_CURRENT_USER,
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            ),
        ];
        for (hive, path) in KEYS {
            mix_reg_key(&mut hash, *hive, path);
        }
    }

    hash
}

/// Fold a registry key's last-write time and subkey count into the hash.
///
/// A key that does not exist is a stable state, exactly like a missing folder
/// in [`mix_path`]: `HKCU\...\Uninstall` is only created once a per-user
/// installer has run, so a fresh profile (or a CI runner) simply has none.
/// Treating that as volatile made every call disagree with the previous one,
/// which forced a full rescan on every 15-minute tick and failed
/// `fingerprint_is_stable_between_calls` on GitHub's runners. Only an error
/// that hides real state — access denied, a transient registry failure —
/// stays volatile, so a permissions hiccup can never suppress a scan.
#[cfg(windows)]
fn mix_reg_key(hash: &mut u64, hive: isize, path: &str) {
    use std::io::ErrorKind;
    use winreg::enums::KEY_READ;
    use winreg::RegKey;

    match RegKey::predef(hive)
        .open_subkey_with_flags(path, KEY_READ)
        .and_then(|k| k.query_info())
    {
        Ok(info) => {
            // `FileTime`'s inner value is private, so go through the
            // SYSTEMTIME accessor winreg exposes.
            let t = info.get_last_write_time_system();
            for part in [
                t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds,
            ] {
                mix(hash, &part.to_le_bytes());
            }
            mix(hash, &(info.sub_keys as u64).to_le_bytes());
        }
        Err(e) if e.kind() == ErrorKind::NotFound => mix(hash, b"missing"),
        Err(_) => mix(hash, &volatile().to_le_bytes()),
    }
}

/// A value that differs on every call, used to force "changed" when a source
/// cannot be read.
fn volatile() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_stable_between_calls() {
        // Two calls with nothing installed/removed in between must agree,
        // otherwise the frontend would rescan every time anyway.
        assert_eq!(compute(), compute());
    }

    #[test]
    fn mix_path_distinguishes_missing_from_present() {
        let mut a: u64 = 0;
        let mut b: u64 = 0;
        mix_path(&mut a, Path::new("Z:\\meteor-does-not-exist"));
        mix_path(&mut b, &std::env::temp_dir());
        assert_ne!(a, b);
    }

    /// Regression: a registry key that does not exist (`HKCU\...\Uninstall` on
    /// a fresh profile or a CI runner) must fold in a *stable* marker, not a
    /// timestamp — otherwise the fingerprint changes on every call and the
    /// "skip the scan when nothing changed" optimisation never fires.
    #[cfg(windows)]
    #[test]
    fn a_missing_registry_key_is_a_stable_state() {
        use winreg::enums::HKEY_CURRENT_USER;
        const MISSING: &str = r"SOFTWARE\Meteor\fingerprint-test\does-not-exist";
        let mut a: u64 = 0;
        let mut b: u64 = 0;
        mix_reg_key(&mut a, HKEY_CURRENT_USER, MISSING);
        mix_reg_key(&mut b, HKEY_CURRENT_USER, MISSING);
        assert_eq!(a, b);

        let mut present: u64 = 0;
        mix_reg_key(&mut present, HKEY_CURRENT_USER, r"SOFTWARE\Microsoft");
        assert_ne!(a, present, "missing and present keys must not collide");
    }
}
