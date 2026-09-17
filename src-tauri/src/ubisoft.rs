// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

use crate::models::Game;

/// Scan Ubisoft Connect installed games from the registry.
///
/// Ubisoft stores one key per game under
/// `HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs\<id>` with only an
/// `InstallDir` value. The display name isn't stored, so we derive it from the
/// install folder. Games launch through the `uplay://` protocol.
#[cfg(windows)]
pub fn scan() -> Result<Vec<Game>, String> {
    use crate::models::GameSource;
    use std::path::Path;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let installs = match hklm.open_subkey(r"SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs") {
        Ok(key) => key,
        Err(_) => return Ok(Vec::new()),
    };

    let mut games = Vec::new();
    for id in installs.enum_keys().flatten() {
        let Ok(entry) = installs.open_subkey(&id) else {
            continue;
        };
        let dir: String = entry.get_value("InstallDir").unwrap_or_default();
        if dir.trim().is_empty() {
            continue;
        }

        let name = Path::new(dir.trim_end_matches(['\\', '/']))
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("Ubisoft {id}"));

        games.push(Game {
            id: format!("ubisoft:{id}"),
            name,
            source: GameSource::Ubisoft,
            app_id: None,
            executable: None,
            install_dir: Some(dir),
            cover_url: None,
            launch_uri: Some(format!("uplay://launch/{id}/0")),
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

