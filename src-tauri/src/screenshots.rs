// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

use crate::models::Game;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png"];

/// Collect the user's **own** screenshots for a game (not promotional art):
/// - Steam: `<Steam>/userdata/<account>/760/remote/<appid>/screenshots/*.jpg`
/// - Windows Game Bar: `<Videos>/Captures/<game name> ….png` (matched by name)
///
/// Returns local file paths, each allowed in the asset-protocol scope at runtime
/// so the webview can render them via `convertFileSrc`. Newest first.
pub fn user_screenshots(app: &AppHandle, game: &Game) -> Vec<String> {
    let mut found: Vec<PathBuf> = Vec::new();

    if let Some(app_id) = game.app_id {
        collect_steam(app_id, &mut found);
    }
    if let Ok(videos) = app.path().video_dir() {
        collect_gamebar(&videos.join("Captures"), &game.name, &mut found);
    }

    // Newest first by modified time.
    found.sort_by_key(|p| {
        std::cmp::Reverse(
            p.metadata()
                .and_then(|m| m.modified())
                .ok()
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
        )
    });

    let scope = app.asset_protocol_scope();
    found
        .into_iter()
        .map(|p| {
            let _ = scope.allow_file(&p);
            p.to_string_lossy().to_string()
        })
        .collect()
}

fn is_image(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Steam stores per-app screenshots under each logged-in account's userdata.
fn collect_steam(app_id: u32, out: &mut Vec<PathBuf>) {
    let Ok(steam) = steamlocate::SteamDir::locate() else {
        return;
    };
    let userdata = steam.path().join("userdata");
    let Ok(accounts) = std::fs::read_dir(&userdata) else {
        return;
    };
    for account in accounts.flatten() {
        let dir = account
            .path()
            .join("760")
            .join("remote")
            .join(app_id.to_string())
            .join("screenshots");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // Only top-level images (the `thumbnails` subfolder is skipped).
            if path.is_file() && is_image(&path) {
                out.push(path);
            }
        }
    }
}

/// Windows Game Bar saves captures named "<window title> <timestamp>.png".
/// Match files whose name starts with the game's name (case-insensitive).
fn collect_gamebar(captures: &std::path::Path, name: &str, out: &mut Vec<PathBuf>) {
    let needle = name.trim().to_lowercase();
    if needle.is_empty() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(captures) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || !is_image(&path) {
            continue;
        }
        let fname = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_lowercase();
        if fname.starts_with(&needle) {
            out.push(path);
        }
    }
}
