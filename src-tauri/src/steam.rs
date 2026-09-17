// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

use crate::models::{Game, GameSource};
use steamlocate::SteamDir;

/// AppIDs that are tools/redistributables, not games.
const BLOCKLIST: &[u32] = &[
    228980, // Steamworks Common Redistributables
];

/// Scan every Steam library folder and return the installed games.
///
/// Errors from individual libraries/apps are skipped rather than aborting the
/// whole scan, so one corrupt manifest can't hide the rest of the collection.
pub fn scan() -> Result<Vec<Game>, String> {
    let steam_dir = SteamDir::locate().map_err(|e| format!("No se encontró Steam: {e}"))?;
    let mut games = Vec::new();

    let libraries = steam_dir
        .libraries()
        .map_err(|e| format!("No se pudieron leer las librerías de Steam: {e}"))?;

    for library in libraries {
        let Ok(library) = library else { continue };

        for app in library.apps() {
            let Ok(app) = app else { continue };

            if BLOCKLIST.contains(&app.app_id) {
                continue;
            }

            let name = match &app.name {
                Some(n) if !n.trim().is_empty() => n.clone(),
                _ => app.install_dir.clone(),
            };

            let lower = name.to_lowercase();
            if lower.contains("proton")
                || lower.contains("steam linux runtime")
                || lower.contains("steamworks")
            {
                continue;
            }

            let install_dir = library
                .resolve_app_dir(&app)
                .to_string_lossy()
                .to_string();

            games.push(Game {
                id: format!("steam:{}", app.app_id),
                name,
                source: GameSource::Steam,
                app_id: Some(app.app_id),
                executable: None,
                install_dir: Some(install_dir),
                // Covers come from IGDB (resolved lazily by the frontend), same
                // as every other source.
                cover_url: None,
                launch_uri: None,
                favorite: false,
                categories: Vec::new(),

            });
        }
    }

    Ok(games)
}

