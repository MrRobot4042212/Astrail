// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! What the app reports about itself and the machine: system info, about
//! and legal texts, diagnostics export, frontend error reports and the access
//! the privileged metrics have.

use crate::*;

#[tauri::command(async)]
pub(crate) fn system_info() -> Result<system::SystemInfo, String> {
    Ok(system::collect())
}

/// Version, author and license of this build, for Settings → About.
#[tauri::command]
pub(crate) fn about_info() -> about::AboutInfo {
    about::info()
}

/// The full text of one of the embedded legal documents.
// Async: the notices are half a megabyte, and serializing them should not sit
// on the main thread.
#[tauri::command(async)]
pub(crate) fn legal_document(name: String) -> Result<&'static str, String> {
    about::document(&name)
}

/// Overlay MPO diagnostics: live composition health + the system-config levers
/// (monitor count, mixed refresh, HAGS) that decide whether the HUD can run on a
/// hardware overlay plane (free) or gets composited by DWM (costing the game's FPS).
#[tauri::command(async)]
pub(crate) fn overlay_mpo_diagnostics() -> system::MpoDiagnostics {
    system::mpo_diagnostics()
}

/// Write one text file with everything needed to look into a bug report (build,
/// hardware, overlay/MPO state, settings, crash reports, the end of the log) and
/// show it in the file manager. Nothing leaves the machine: the user reads it and
/// decides whether to attach it. Returns the path.
#[tauri::command]
pub(crate) async fn export_diagnostics(app: AppHandle) -> Result<String, String> {
    blocking(move || {
        let settings = current_settings(&app);
        let header = diagnostics_header(&settings);
        let path = applog::write_diagnostics(&header)?;
        log::info!("diagnostics exported to {}", path.display());
        if let Some(dir) = path.parent() {
            files::open_folder(dir)?;
        }
        Ok(path.to_string_lossy().into_owned())
    })
    .await
}

pub(crate) fn diagnostics_header(settings: &AppSettings) -> String {
    fn json<T: serde::Serialize>(value: &T) -> String {
        serde_json::to_string_pretty(value).unwrap_or_else(|e| format!("<not serializable: {e}>"))
    }
    format!(
        "Astrail {} diagnostics\ntime: {}\nmetrics access: {:?}\n\n===== system =====\n{}\n\n===== overlay / MPO =====\n{}\n\n===== settings =====\n{}\n",
        env!("CARGO_PKG_VERSION"),
        applog::timestamp(
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
        ),
        metrics_access(),
        json(&system::collect()),
        json(&system::mpo_diagnostics()),
        json(settings),
    )
}

/// A render error or an unhandled rejection caught in the webview. The webview is
/// not trusted: the text is flattened to one bounded line, and a burst is dropped
/// after the first few so a render loop cannot fill the log.
// Async: a log line is a file write.
#[tauri::command(async)]
pub(crate) fn report_frontend_error(window: tauri::Window, message: String) {
    use std::sync::atomic::{AtomicU32, Ordering};
    const MAX_PER_RUN: u32 = 50;
    static SEEN: AtomicU32 = AtomicU32::new(0);
    let seen = SEEN.fetch_add(1, Ordering::Relaxed);
    if seen < MAX_PER_RUN {
        log::error!(
            target: "astrail::webview",
            "[{}] {}",
            window.label(),
            applog::sanitize_frontend_message(&message)
        );
    } else if seen == MAX_PER_RUN {
        log::error!(target: "astrail::webview", "further webview errors are not logged in this run");
    }
}

/// The current OS user's name, for greetings. Prefers the Windows display/full
/// name (e.g. "Diego Chicoma"); falls back to the login name (USERNAME). Empty
/// string if nothing is available.
// Async: `GetUserNameExW(NameDisplay)` can round-trip to a domain controller.
#[tauri::command(async)]
pub(crate) fn username() -> String {
    #[cfg(windows)]
    {
        if let Some(name) = system::display_name() {
            return name;
        }
    }
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
}

/// What the privileged metrics can use in this process, for the settings screen.
///
/// Each field is a fact the UI turns into advice: FPS through PresentMon needs
/// `etw` (admin *or* Performance Log Users), CPU temperature needs `elevated`
/// *and* `pawnio`. Group membership is fixed at logon; PawnIO can be installed
/// while Astrail runs, so the screen re-asks each time it opens.
#[derive(serde::Serialize, Clone, Copy, Debug)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct MetricsAccess {
    pub elevated: bool,
    pub etw: bool,
    pub pawnio: bool,
}

/// Whether Astrail is running elevated (admin). False on non-Windows.
pub(crate) fn is_elevated() -> bool {
    #[cfg(windows)]
    {
        elevation::is_elevated()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[tauri::command(async)]
pub(crate) fn metrics_access() -> MetricsAccess {
    #[cfg(windows)]
    {
        MetricsAccess {
            elevated: elevation::is_elevated(),
            etw: elevation::can_trace_etw(),
            pawnio: cputemp::pawnio_installed(),
        }
    }
    #[cfg(not(windows))]
    {
        MetricsAccess { elevated: false, etw: false, pawnio: false }
    }
}
