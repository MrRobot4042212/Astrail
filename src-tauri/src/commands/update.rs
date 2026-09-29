// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Leaving the process: installing an update, restarting elevated, and the
//! shutdown shared with a normal exit.

use super::settings::flush_settings;
use crate::blocking;
use crate::error::CmdResult;
#[cfg(windows)]
use crate::elevation;
use tauri::AppHandle;

/// Flush pending state and stop both sidecars properly. Termination is not a
/// shutdown for either of them: cputemp would leave the LibreHardwareMonitor kernel
/// driver loaded and registered for the rest of the boot, and PresentMon would leave
/// its ETW realtime session live with its buffers pinned until reboot. The
/// kill-on-close job (`jobobj.rs`) stays as the crash backstop, which is all it can
/// be — TerminateProcess cannot be intercepted.
pub(crate) fn shutdown_for_exit(app: &AppHandle) {
    // Stand the controllers down first: a sidecar exit they reap from here on is
    // this shutdown, not a crash to back off from and warn about.
    crate::metrics::set_sidecars_suspended(true);
    // The URL cache is written at most every 2 s during a cover pass; make sure the
    // last entries are not lost.
    crate::art::flush(app);
    // So is an overlay toggle whose write was still running off the main thread.
    flush_settings(app);
    #[cfg(windows)]
    {
        crate::presentmon::shutdown();
        crate::cputemp::shutdown();
    }
}

/// Called right before the updater installs. The plugin's `install` ends in
/// `std::process::exit`, so `RunEvent::Exit` never fires on that path; without this
/// an update taken with admin metrics on left the ETW session and the kernel driver
/// behind. The sidecars stay suspended until `abort_update` (the install failed) or
/// the process exits.
#[tauri::command]
pub(crate) async fn prepare_for_update(app: AppHandle) -> CmdResult<()> {
    log::info!("update downloaded: stopping the sidecars before the installer runs");
    crate::metrics::set_sidecars_suspended(true);
    blocking(move || {
        shutdown_for_exit(&app);
        Ok(())
    })
    .await
}

/// The install did not happen after `prepare_for_update`: let the sidecars run again.
#[tauri::command]
pub(crate) fn abort_update() {
    log::warn!("update install did not happen: sidecars allowed again");
    crate::metrics::set_sidecars_suspended(false);
}

/// Relaunch Astrail as administrator (UAC prompt), then exit this instance.
///
/// `async` through `blocking()`: `ShellExecuteW("runas")` only returns once the
/// UAC prompt is answered, and on the main thread that froze the window, the tray
/// and the hotkeys for as long as the prompt stayed open.
#[tauri::command]
pub(crate) async fn restart_as_admin(app: AppHandle) -> CmdResult<()> {
    #[cfg(windows)]
    {
        blocking(elevation::relaunch_elevated).await?;
        // Let the command response flush, then quit so only the elevated copy runs.
        let handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(400));
            handle.exit(0);
        });
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Err("Only available on Windows.".into())
    }
}
