// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! "Start with Windows": the per-user `Run` registry key, nothing else.
//!
//! Replaces `tauri-plugin-autostart`, whose `auto-launch` backend wrote the
//! executable path **unquoted** (a path with spaces is split by the shell at the
//! first one), left the `StartupApproved\Run` entry that Task Manager writes
//! behind on disable, and dragged a second `winreg` version into the graph.
//! Meteor never starts itself elevated at logon (see
//! `elevation::remove_legacy_logon_task`), so a plain user key is the whole
//! feature.

/// Value name under both keys — what Task Manager's "Startup apps" shows.
pub const VALUE_NAME: &str = "Meteor";

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Task Manager records the user's enable/disable choice here as a binary
/// blob: first byte `0x02` = enabled, `0x03` = disabled. It is consulted by the
/// shell at logon, so a stale "disabled" entry silently overrides our Run value.
#[cfg(windows)]
const APPROVED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// The command line stored in the Run key: the executable quoted, so a path
/// containing a space is not split by the shell at logon.
pub(crate) fn run_value(exe: &std::path::Path) -> String {
    format!("\"{}\"", exe.display())
}

/// Whether a `StartupApproved` blob marks the entry as enabled. A missing or
/// malformed blob counts as enabled, which is what the shell does too.
pub(crate) fn approved_enabled(blob: &[u8]) -> bool {
    blob.first().is_none_or(|b| b % 2 == 0)
}

#[cfg(windows)]
mod imp {
    use super::{approved_enabled, run_value, APPROVED_KEY, RUN_KEY, VALUE_NAME};
    use std::io;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    use winreg::{RegKey, RegValue};

    fn open(path: &str, write: bool) -> io::Result<RegKey> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if write {
            hkcu.create_subkey(path).map(|(k, _)| k)
        } else {
            hkcu.open_subkey_with_flags(path, KEY_READ)
        }
    }

    fn ignore_missing(r: io::Result<()>) -> io::Result<()> {
        match r {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    fn expected() -> io::Result<String> {
        std::env::current_exe().map(|exe| run_value(&exe))
    }

    /// Enabled = a Run value exists and Task Manager has not disabled it.
    pub fn is_enabled() -> io::Result<bool> {
        let run = match open(RUN_KEY, false) {
            Ok(k) => k,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        if run.get_value::<String, _>(VALUE_NAME).is_err() {
            return Ok(false);
        }
        let approved = match open(APPROVED_KEY, false) {
            Ok(k) => k,
            Err(_) => return Ok(true),
        };
        Ok(approved
            .get_raw_value(VALUE_NAME)
            .map(|v: RegValue| approved_enabled(&v.bytes))
            .unwrap_or(true))
    }

    /// Write the quoted path of the running executable. Idempotent, and it
    /// repairs an unquoted or stale value from an older build; clearing the
    /// `StartupApproved` entry re-enables the item if Task Manager disabled it.
    pub fn enable() -> io::Result<()> {
        let value = expected()?;
        open(RUN_KEY, true)?.set_value(VALUE_NAME, &value)?;
        if let Ok(approved) = open(APPROVED_KEY, true) {
            ignore_missing(approved.delete_value(VALUE_NAME))?;
        }
        Ok(())
    }

    /// Remove both entries. Missing values are not an error.
    pub fn disable() -> io::Result<()> {
        if let Ok(run) = open(RUN_KEY, true) {
            ignore_missing(run.delete_value(VALUE_NAME))?;
        }
        if let Ok(approved) = open(APPROVED_KEY, true) {
            ignore_missing(approved.delete_value(VALUE_NAME))?;
        }
        Ok(())
    }

    /// Startup self-heal: if autostart is on but the stored command line is not
    /// the quoted path of this executable (an unquoted value from the old
    /// plugin, or a moved install), rewrite it. One key read when it matches.
    pub fn repair() -> io::Result<()> {
        let Ok(run) = open(RUN_KEY, false) else { return Ok(()) };
        let Ok(current) = run.get_value::<String, _>(VALUE_NAME) else {
            return Ok(());
        };
        if current != expected()? {
            open(RUN_KEY, true)?.set_value(VALUE_NAME, &expected()?)?;
        }
        Ok(())
    }
}

#[cfg(windows)]
pub use imp::{disable, enable, is_enabled, repair};

#[cfg(not(windows))]
mod imp {
    use std::io;
    fn unsupported() -> io::Error {
        io::Error::new(io::ErrorKind::Unsupported, "autostart is Windows-only")
    }
    pub fn is_enabled() -> io::Result<bool> {
        Err(unsupported())
    }
    pub fn enable() -> io::Result<()> {
        Err(unsupported())
    }
    pub fn disable() -> io::Result<()> {
        Err(unsupported())
    }
    pub fn repair() -> io::Result<()> {
        Ok(())
    }
}

#[cfg(not(windows))]
pub use imp::{disable, enable, is_enabled, repair};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_run_value_is_quoted() {
        // Regression (BD7): the previous backend wrote the path bare, and this
        // machine's key held `C:\Users\me\AppData\Local\Meteor\meteor.exe ` with
        // a trailing space — a path with a space in it would be cut there.
        assert_eq!(
            run_value(Path::new(r"C:\Users\Jane Doe\AppData\Local\Meteor\meteor.exe")),
            r#""C:\Users\Jane Doe\AppData\Local\Meteor\meteor.exe""#
        );
    }

    #[test]
    fn task_manager_disable_is_respected() {
        assert!(approved_enabled(&[0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert!(!approved_enabled(&[0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert!(approved_enabled(&[]));
        assert!(approved_enabled(&[0x06]));
    }
}
