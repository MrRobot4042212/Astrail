// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

use crate::models::Game;

/// Scan GOG / GOG Galaxy installed games from the registry.
///
/// GOG records every installed game under
/// `HKLM\SOFTWARE\WOW6432Node\GOG.com\Games\<gameID>` (or without `WOW6432Node`
/// on 32-bit Windows), including the display name, install path and exe.
#[cfg(windows)]
pub fn scan() -> Result<Vec<Game>, String> {
    use crate::models::GameSource;
    use std::path::PathBuf;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let games_key = match hklm
        .open_subkey(r"SOFTWARE\WOW6432Node\GOG.com\Games")
        .or_else(|_| hklm.open_subkey(r"SOFTWARE\GOG.com\Games"))
    {
        Ok(key) => key,
        // GOG not installed: degrade to an empty list.
        Err(_) => return Ok(Vec::new()),
    };

    let mut games = Vec::new();
    for id in games_key.enum_keys().flatten() {
        let Ok(entry) = games_key.open_subkey(&id) else {
            continue;
        };

        let name: String = entry.get_value("gameName").unwrap_or_default();
        if name.trim().is_empty() {
            continue;
        }
        let path: String = entry.get_value("path").unwrap_or_default();

        // `exe` is usually the full path; fall back to path + exeFile.
        let mut exe: String = entry.get_value("exe").unwrap_or_default();
        if exe.trim().is_empty() {
            let exe_file: String = entry.get_value("exeFile").unwrap_or_default();
            if !path.is_empty() && !exe_file.is_empty() {
                exe = PathBuf::from(&path)
                    .join(exe_file)
                    .to_string_lossy()
                    .to_string();
            }
        }

        games.push(Game {
            id: format!("gog:{id}"),
            name,
            source: GameSource::Gog,
            app_id: None,
            executable: (!exe.trim().is_empty()).then_some(exe),
            install_dir: (!path.trim().is_empty()).then_some(path),
            cover_url: None,
            launch_uri: None,
            favorite: false,
            categories: Vec::new(),

        });
    }

    Ok(games)
}

#[cfg(not(windows))]
pub fn scan() -> Result<Vec<Game>, String> {
    Ok(Vec::new())
}

