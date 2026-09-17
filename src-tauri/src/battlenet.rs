// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

use crate::models::Game;

/// Scan installed World of Warcraft *flavors* (Retail / Classic / Classic Era).
///
/// Battle.net keeps every WoW flavor inside one install root (`World of
/// Warcraft\`), each in a fixed subfolder (`_retail_`, `_classic_`,
/// `_classic_era_`, …) with its own executable. The registry only records a
/// single "World of Warcraft" uninstall entry, so to tell the flavors apart we
/// locate that root and enumerate which flavor subfolders are present. Each is
/// emitted as its own `Game` and launched through the Battle.net protocol with
/// the flavor's product code, so the client opens the right version.
///
/// PTR/test environments (`_ptr_`, `_classic_ptr_`, `_xptr_`) are intentionally
/// skipped — noise for most users.
#[cfg(windows)]
pub fn scan() -> Result<Vec<Game>, String> {
    use crate::models::GameSource;

    let Some(root) = find_wow_root() else {
        return Ok(Vec::new());
    };

    // (subfolder, display name, executable, Battle.net product code, id suffix)
    const FLAVORS: [(&str, &str, &str, &str, &str); 3] = [
        ("_retail_", "World of Warcraft", "Wow.exe", "WoW", "retail"),
        (
            "_classic_",
            "World of Warcraft Classic",
            "WowClassic.exe",
            "WoW_classic",
            "classic",
        ),
        (
            "_classic_era_",
            "World of Warcraft Classic Era",
            "WowClassicEra.exe",
            "WoW_classic_era",
            "classic_era",
        ),
    ];

    let mut games = Vec::new();
    for (subdir, name, exe, code, id) in FLAVORS {
        let flavor_dir = root.join(subdir);
        if !flavor_dir.is_dir() {
            continue;
        }
        let exe_path = flavor_dir.join(exe);
        games.push(Game {
            id: format!("battlenet:wow_{id}"),
            name: name.to_string(),
            source: GameSource::Battlenet,
            app_id: None,
            // Direct-exe fallback if the protocol launch fails.
            executable: exe_path
                .exists()
                .then(|| exe_path.to_string_lossy().to_string()),
            install_dir: Some(flavor_dir.to_string_lossy().to_string()),
            cover_url: None,
            launch_uri: Some(format!("battlenet://{code}")),
            favorite: false,
            categories: Vec::new(),

        });
    }

    Ok(games)
}

/// Locate the World of Warcraft install root: prefer the Battle.net uninstall
/// entry's `InstallLocation`, then fall back to the usual Program Files paths.
#[cfg(windows)]
fn find_wow_root() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    for key in [
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\World of Warcraft",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\World of Warcraft",
    ] {
        if let Ok(k) = hklm.open_subkey(key) {
            let loc: String = k.get_value("InstallLocation").unwrap_or_default();
            let loc = loc.trim();
            if !loc.is_empty() {
                let p = PathBuf::from(loc);
                if p.is_dir() {
                    return Some(p);
                }
            }
        }
    }

    for base in [
        r"C:\Program Files (x86)\World of Warcraft",
        r"C:\Program Files\World of Warcraft",
    ] {
        let p = PathBuf::from(base);
        if p.is_dir() {
            return Some(p);
        }
    }

    None
}

#[cfg(not(windows))]
pub fn scan() -> Result<Vec<Game>, String> {
    Ok(Vec::new())
}

