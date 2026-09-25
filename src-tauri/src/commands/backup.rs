// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! User data backups: export, pick (validate + summarise), apply, discard.

use crate::*;

/// The backup the user picked and saw the summary of, until they confirm or cancel.
///
/// The path and the hash stay on this side: the webview is never trusted with a
/// file to read or write (both dialogs are opened from Rust), and the restore
/// refuses a file whose bytes are not the ones the summary was made from.
#[derive(Default)]
pub(crate) struct PendingImport(std::sync::Mutex<Option<(std::path::PathBuf, [u8; 32])>>);

impl PendingImport {
    fn replace(&self, value: Option<(std::path::PathBuf, [u8; 32])>) -> Option<(std::path::PathBuf, [u8; 32])> {
        std::mem::replace(&mut *self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner), value)
    }
}

/// A file dialog owned by the main window. Blocking: only call it from `blocking`.
pub(crate) fn backup_dialog(app: &AppHandle) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
    use tauri_plugin_dialog::DialogExt;
    let dialog = app.dialog().file().add_filter("Astrail backup", &["json"]);
    match app.get_webview_window("main") {
        Some(window) => dialog.set_parent(&window),
        None => dialog,
    }
}

/// Save the data only the user can recreate (manual apps, play time, favorites,
/// categories, hidden entries, chosen covers, settings) to a file they pick.
/// `None` = they closed the dialog. See `backup.rs` for what is in the file.
#[tauri::command]
pub(crate) async fn export_user_data(app: AppHandle) -> Result<Option<backup::ExportReport>, String> {
    blocking(move || {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        // `YYYY-MM-DD`: the timestamp is ASCII, so the slice cannot split a char.
        let day = applog::timestamp(now.as_millis()).get(..10).unwrap_or("data").to_string();
        let Some(target) = backup_dialog(&app)
            .set_file_name(format!("astrail-backup-{day}.json"))
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let target = target.into_path().map_err(|e| format!("Unusable backup path: {e}"))?;
        let report = backup::export(&jsonstore::data_dir(&app)?, &target, now.as_secs())?;
        log::info!(
            "user data exported to {} ({} cover(s), {} skipped, {} unreadable store(s))",
            target.display(),
            report.covers,
            report.covers_skipped,
            report.unreadable.len()
        );
        Ok(Some(report))
    })
    .await
}

/// Let the user pick a backup and validate it. Nothing is written: the summary is
/// what they confirm, and `apply_user_data_backup` does the restore.
#[tauri::command]
pub(crate) async fn pick_user_data_backup(app: AppHandle) -> Result<Option<backup::ImportSummary>, String> {
    blocking(move || {
        let pending = app.state::<PendingImport>();
        pending.replace(None);
        // Start where the automatic and pre-import copies live, when it exists.
        let mut dialog = backup_dialog(&app);
        if let Ok(folder) = jsonstore::data_dir(&app).map(|d| d.join(backup::SAFETY_DIR)) {
            if folder.is_dir() {
                dialog = dialog.set_directory(folder);
            }
        }
        let Some(picked) = dialog.blocking_pick_file() else {
            return Ok(None);
        };
        let path = picked.into_path().map_err(|e| format!("Unusable backup path: {e}"))?;
        let (restore, hash) = backup::read(&path)?;
        pending.replace(Some((path, hash)));
        Ok(Some(restore.summary))
    })
    .await
}

/// Forget the picked backup (the user cancelled, or closed the settings).
#[tauri::command]
pub(crate) fn discard_user_data_backup(pending: tauri::State<'_, PendingImport>) {
    pending.replace(None);
}

/// Replace the user's data with the backup picked in `pick_user_data_backup`. The
/// data being replaced is saved under `backups/` first; without that copy nothing
/// is touched.
#[tauri::command]
pub(crate) async fn apply_user_data_backup(app: AppHandle) -> Result<backup::ImportReport, String> {
    blocking(move || {
        let (path, hash) = app
            .state::<PendingImport>()
            .replace(None)
            .ok_or_else(|| "No backup is waiting to be imported".to_string())?;
        let restore = backup::read_verified(&path, &hash)?;
        let dir = jsonstore::data_dir(&app)?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let safety = backup::write_safety_copy(&dir, now)
            .map_err(|e| format!("Nothing was imported: the current data could not be saved first ({e})"))?;
        let safety = safety.to_string_lossy().into_owned();

        let outcome = backup::apply(&dir, &restore).and_then(|covers| {
            restore_settings(&app, restore.settings.clone())?;
            Ok(covers)
        });
        if let Some(id) = restore.discord_id() {
            discord::set_client_id(id);
        }
        // Even after a failure: some stores may already hold the restored data.
        events::user_data_imported(&app);
        events::playtime_updated(&app, None);
        match outcome {
            Ok(covers) => {
                log::info!("user data imported from a backup ({covers} cover(s)); previous data in {safety}");
                Ok(backup::ImportReport { covers, safety_copy: safety })
            }
            Err(e) => {
                log::error!("user data import stopped half way: {e}; previous data in {safety}");
                Err(format!("{e}. The import is incomplete; your previous data is in {safety}"))
            }
        }
    })
    .await
}

/// Swap in the settings of a backup: same lock, same order as `patch_app_settings`.
pub(crate) fn restore_settings(app: &AppHandle, settings: Option<AppSettings>) -> Result<(), String> {
    let (Some(mut next), Some(state)) = (settings, app.try_state::<std::sync::Mutex<AppSettings>>()) else {
        return Ok(());
    };
    let previous = {
        let mut current = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // A backup made before the first-run setup was finished must not bring
        // the setup back on a machine that already went through it.
        next.setup_completed |= current.setup_completed;
        // Not `storage::save_settings`, which logs a failed write and carries on:
        // here the user is told whether their settings were restored.
        jsonstore::save_if_changed(app, storage::SETTINGS_FILE, &next)?;
        std::mem::replace(&mut *current, next.clone())
    };
    settings_changed(app, &previous, &next);
    Ok(())
}
