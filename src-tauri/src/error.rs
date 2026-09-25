// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! The error every command returns to the webview.
//!
//! Commands used to return `Result<_, String>`: an English sentence the frontend
//! could only show as it came, untranslated, and could not tell apart from any
//! other failure. `AppError` carries a stable `code` the webview translates
//! (`src/lib/errors.ts`, keys `errors.<code>` in both catalogs) and the English
//! `detail`, which is what the log and the diagnostics export keep.
//!
//! Internal helpers keep returning `String`; a command attaches the code at its
//! boundary. A plain `String` becomes `Internal`, so `?` keeps working.

use serde::Serialize;

/// What went wrong, in terms the user can act on. Serialized in snake_case; the
/// webview has a translation for each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum ErrorCode {
    /// An id or file the command was given does not exist (any more).
    NotFound,
    /// A value from the webview was refused (unknown setting, bad name, bad URL).
    InvalidInput,
    /// Autostart can only point at an installed copy.
    AutostartUnavailable,
    /// The chosen file is not a usable Astrail backup.
    BackupInvalid,
    /// The backup file changed between choosing it and restoring it.
    BackupChanged,
    /// The game or app could not be started.
    LaunchFailed,
    /// Something outside Astrail failed: disk, registry, network, a process.
    Io,
    /// A bug or an unexpected state; the detail says which.
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct AppError {
    pub code: ErrorCode,
    /// English, for the log and the diagnostics export; shown to the user only
    /// when there is no translation for `code`.
    pub detail: String,
}

impl AppError {
    pub fn new(code: ErrorCode, detail: impl Into<String>) -> Self {
        Self { code, detail: detail.into() }
    }

    /// Wrap an internal `String` error with the code that fits this boundary.
    pub fn with(code: ErrorCode) -> impl FnOnce(String) -> Self {
        move |detail| Self::new(code, detail)
    }
}

impl From<String> for AppError {
    fn from(detail: String) -> Self {
        Self::new(ErrorCode::Internal, detail)
    }
}

impl From<&str> for AppError {
    fn from(detail: &str) -> Self {
        Self::new(ErrorCode::Internal, detail)
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.detail)
    }
}

/// The result every command returns.
pub type CmdResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_webview_receives_a_code_and_the_english_detail() {
        let e = AppError::new(ErrorCode::AutostartUnavailable, "not installed");
        assert_eq!(
            serde_json::to_value(&e).expect("serializable"),
            serde_json::json!({ "code": "autostart_unavailable", "detail": "not installed" })
        );
        // A bare string from an internal helper is an internal error, not lost.
        let internal: AppError = String::from("boom").into();
        assert_eq!(internal.code, ErrorCode::Internal);
        assert_eq!(internal.detail, "boom");
    }

    #[test]
    fn no_spanish_text_is_written_in_the_core() {
        // The detail of an error reaches the screen inside the translated message
        // and the log, so it has to be English whatever the UI language is. These
        // markers caught the last ones ("No se pudo abrir «…»", "Desconocido").
        const MARKERS: &[&str] = &["«", "»", "¿", "¡", "No se ", "no se pudo", "Desconocid", "vacío", "carpeta"];
        let mut dirs = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        let mut found = Vec::new();
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).expect("source dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    dirs.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") || path.ends_with("error.rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("source file");
                let code = text.split("#[cfg(test)]").next().unwrap_or_default();
                for (n, line) in code.lines().enumerate() {
                    let trimmed = line.trim_start();
                    if trimmed.starts_with("//") {
                        continue;
                    }
                    if MARKERS.iter().any(|m| line.contains(m)) {
                        found.push(format!("{}:{}: {}", path.display(), n + 1, trimmed));
                    }
                }
            }
        }
        assert!(found.is_empty(), "Spanish text in the core:\n{}", found.join("\n"));
    }
}
