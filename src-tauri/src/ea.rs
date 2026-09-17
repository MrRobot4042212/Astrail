// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

use crate::models::Game;

/// Scan EA app / Origin installed games from the registry.
///
/// Both the current EA app and legacy Origin register games under
/// `HKLM\SOFTWARE\WOW6432Node\EA Games\<title>` (or `Origin Games`) with an
/// `Install Dir`. There is no reliable offline launch id, so we launch the best
/// candidate executable found in the install folder directly.
#[cfg(windows)]
pub fn scan() -> Result<Vec<Game>, String> {
    use crate::models::GameSource;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let mut games = Vec::new();

    for root in [
        r"SOFTWARE\WOW6432Node\EA Games",
        r"SOFTWARE\WOW6432Node\Origin Games",
    ] {
        let Ok(key) = hklm.open_subkey(root) else {
            continue;
        };

        for title in key.enum_keys().flatten() {
            let Ok(entry) = key.open_subkey(&title) else {
                continue;
            };

            let dir: String = entry
                .get_value("Install Dir")
                .or_else(|_| entry.get_value("InstallDir"))
                .unwrap_or_default();
            let display: String = entry.get_value("DisplayName").unwrap_or_default();

            let name = if display.trim().is_empty() {
                title.clone()
            } else {
                display
            };
            if name.trim().is_empty() {
                continue;
            }

            games.push(Game {
                id: format!("ea:{title}"),
                name,
                source: GameSource::Ea,
                app_id: None,
                executable: find_launch_exe(&dir),
                install_dir: (!dir.trim().is_empty()).then_some(dir),
                cover_url: None,
                launch_uri: None,
                favorite: false,
                categories: Vec::new(),

            });
        }
    }

    Ok(games)
}

/// Pick a plausible game executable from the top level of `dir`, skipping
/// installers, crash handlers and redistributables.
#[cfg(windows)]
fn find_launch_exe(dir: &str) -> Option<String> {
    if dir.trim().is_empty() {
        return None;
    }
    let skip = ["unins", "setup", "vcredist", "redist", "crash", "touchup", "dxsetup"];
    let mut best: Option<std::path::PathBuf> = None;
    let mut best_size = 0u64;

    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("exe")) != Some(true)
        {
            continue;
        }
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if skip.iter().any(|s| stem.contains(s)) {
            continue;
        }
        // Prefer the largest exe — usually the game itself, not a helper.
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if size >= best_size {
            best_size = size;
            best = Some(path);
        }
    }

    best.map(|p| p.to_string_lossy().to_string())
}

#[cfg(not(windows))]
pub fn scan() -> Result<Vec<Game>, String> {
    Ok(Vec::new())
}

