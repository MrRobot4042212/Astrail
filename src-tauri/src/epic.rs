// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

use crate::models::{Game, GameSource};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

/// One Epic install manifest (`*.item`). Only the fields we need are mapped.
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Manifest {
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    install_location: String,
    #[serde(default)]
    launch_executable: String,
    /// Internal id used by the launcher protocol.
    #[serde(default)]
    app_name: String,
    /// e.g. ["public", "games"] — used to skip engines/plugins/tools.
    #[serde(default)]
    app_categories: Vec<String>,
}

/// `%PROGRAMDATA%\Epic\EpicGamesLauncher\Data\Manifests`.
fn manifests_dir() -> Option<PathBuf> {
    let program_data = std::env::var("PROGRAMDATA").ok()?;
    let dir = PathBuf::from(program_data)
        .join("Epic")
        .join("EpicGamesLauncher")
        .join("Data")
        .join("Manifests");
    dir.is_dir().then_some(dir)
}

/// Scan installed Epic Games Store titles by reading their `.item` manifests.
/// Returns an empty list (not an error) when Epic isn't installed.
pub fn scan() -> Result<Vec<Game>, String> {
    let Some(dir) = manifests_dir() else {
        return Ok(Vec::new());
    };

    let mut games = Vec::new();
    let entries = fs::read_dir(&dir).map_err(|e| format!("No se pudo leer Epic: {e}"))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("item") {
            continue;
        }

        let Ok(data) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(m) = serde_json::from_str::<Manifest>(&data) else {
            continue;
        };

        // Skip non-game entries (Unreal Engine, plugins, tools…).
        if !m.app_categories.iter().any(|c| c == "games") {
            continue;
        }
        if m.display_name.trim().is_empty() || m.app_name.trim().is_empty() {
            continue;
        }

        let executable = if m.install_location.is_empty() || m.launch_executable.is_empty() {
            None
        } else {
            Some(
                PathBuf::from(&m.install_location)
                    .join(&m.launch_executable)
                    .to_string_lossy()
                    .to_string(),
            )
        };

        games.push(Game {
            id: format!("epic:{}", m.app_name),
            name: m.display_name,
            source: GameSource::Epic,
            app_id: None,
            executable,
            install_dir: (!m.install_location.is_empty()).then_some(m.install_location),
            cover_url: None,
            // Launch through the Epic client so it handles auth/cloud saves.
            launch_uri: Some(format!(
                "com.epicgames.launcher://apps/{}?action=launch&silent=true",
                m.app_name
            )),
            favorite: false,
            categories: Vec::new(),

        });
    }

    Ok(games)
}

