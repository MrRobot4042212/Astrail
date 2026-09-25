// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

use crate::models::Game;

/// Battle.net products other than WoW, as `(uid, product code)`. The uid is the
/// `--uid=` its uninstaller is registered with; the code is what
/// `battlenet://<code>` and the client use. Verified against real installs:
/// `hs_beta`, `heroes`, `prometheus` (and `wow`, handled by flavor below). The
/// others follow the same naming in Battle.net's product list but were not
/// installed to check; a wrong code would only affect the protocol fallback,
/// since the executable is tried first (see `launcher::launch`).
const PRODUCTS: &[(&str, &str)] = &[
    ("hs_beta", "WTCG"),
    ("heroes", "Hero"),
    ("prometheus", "Pro"),
    ("s2", "S2"),
    ("s1", "S1"),
    ("diablo3", "D3"),
    ("osi", "OSI"),
    ("w3", "W3"),
    ("fenris", "Fen"),
];

/// The `--uid=` of a Blizzard uninstaller command line, lower-cased.
fn uninstall_uid(uninstall: &str) -> Option<String> {
    if !uninstall.to_ascii_lowercase().contains("blizzard uninstaller.exe") {
        return None;
    }
    uninstall
        .split_whitespace()
        .find_map(|arg| arg.strip_prefix("--uid="))
        .map(|uid| uid.trim_matches('"').to_ascii_lowercase())
        .filter(|uid| !uid.is_empty())
}

/// The product code for an uninstaller's uid, when it is a known game.
fn product_code(uid: &str) -> Option<&'static str> {
    PRODUCTS.iter().find(|(u, _)| *u == uid).map(|(_, code)| *code)
}

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
    let mut games = scan_wow();
    games.extend(scan_products());
    Ok(games)
}

/// Every Battle.net game but WoW, from the uninstall entries the client writes.
///
/// Before this, they only reached the library through the generic registry scan
/// as `windows:` guesses: labelled as generic apps, and not timed when started
/// from the Battle.net client until the user had confirmed them as games.
#[cfg(windows)]
fn scan_products() -> Vec<Game> {
    use crate::models::GameSource;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    const ROOTS: &[(isize, &str)] = &[
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_CURRENT_USER, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    ];
    let mut games: Vec<Game> = Vec::new();
    for (hive, path) in ROOTS {
        let Ok(root) = RegKey::predef(*hive).open_subkey(path) else { continue };
        for sub in root.enum_keys().flatten() {
            let Ok(entry) = root.open_subkey(&sub) else { continue };
            let uninstall: String = entry.get_value("UninstallString").unwrap_or_default();
            let Some(uid) = uninstall_uid(&uninstall) else { continue };
            let Some(code) = product_code(&uid) else { continue };
            let id = format!("battlenet:{uid}");
            if games.iter().any(|g| g.id == id) {
                continue;
            }
            let name: String = entry.get_value("DisplayName").unwrap_or_default();
            let location: String = entry.get_value("InstallLocation").unwrap_or_default();
            let location = location.trim().trim_matches('"');
            let icon: String = entry.get_value("DisplayIcon").unwrap_or_default();
            games.push(Game {
                id,
                name: if name.trim().is_empty() { sub.clone() } else { name.trim().to_string() },
                source: GameSource::Battlenet,
                app_id: None,
                // Same executable the generic scan picked, so launching is unchanged.
                executable: crate::windows_apps::exe_from_icon(&icon),
                install_dir: std::path::Path::new(location).is_dir().then(|| location.to_string()),
                cover_url: None,
                launch_uri: Some(format!("battlenet://{code}")),
                favorite: false,
                categories: Vec::new(),
            });
        }
    }
    games
}

/// Installed WoW flavors, one entry each (see `scan`).
#[cfg(windows)]
fn scan_wow() -> Vec<Game> {
    use crate::models::GameSource;

    let Some(root) = find_wow_root() else {
        return Vec::new();
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

    games
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blizzard_games_are_recognised_by_their_uninstaller_uid() {
        // Regression: Heroes of the Storm only appeared as a `windows:` registry
        // guess. Strings copied from a real install.
        let heroes = r#""C:\ProgramData\Battle.net\Agent\Blizzard Uninstaller.exe" --lang=esES --uid=heroes --displayname="Heroes of the Storm""#;
        let hearthstone = r#""C:\ProgramData\Battle.net\Agent\Blizzard Uninstaller.exe" --lang=esES --uid=hs_beta --displayname="Hearthstone""#;
        let overwatch = r#""C:\ProgramData\Battle.net\Agent\Blizzard Uninstaller.exe" --lang=esES --uid=prometheus --displayname="Overwatch""#;
        assert_eq!(uninstall_uid(heroes).as_deref(), Some("heroes"));
        assert_eq!(product_code("heroes"), Some("Hero"));
        assert_eq!(uninstall_uid(hearthstone).and_then(|u| product_code(&u)), Some("WTCG"));
        assert_eq!(uninstall_uid(overwatch).and_then(|u| product_code(&u)), Some("Pro"));
    }

    #[test]
    fn the_client_itself_wow_and_other_uninstallers_are_not_products() {
        let client = r#""C:\ProgramData\Battle.net\Agent\Blizzard Uninstaller.exe" --lang=esES --uid=battle.net --displayname="Battle.net""#;
        let wow = r#""C:\ProgramData\Battle.net\Agent\Blizzard Uninstaller.exe" --lang=esES --uid=wow --displayname="World of Warcraft""#;
        assert_eq!(uninstall_uid(client).and_then(|u| product_code(&u)), None);
        // WoW is scanned per flavor instead.
        assert_eq!(uninstall_uid(wow).and_then(|u| product_code(&u)), None);
        assert_eq!(uninstall_uid(r#""C:\Games\unins000.exe" --uid=heroes"#), None);
        assert_eq!(uninstall_uid(""), None);
    }

    /// Reads this machine's registry: `cargo test -- --ignored`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn installed_products_are_found_in_the_real_registry() {
        let games = scan_products();
        for g in &games {
            println!("{} | {} | {:?} | {:?} | {:?}", g.id, g.name, g.executable, g.install_dir, g.launch_uri);
            assert!(g.id.starts_with("battlenet:"));
            assert!(g.launch_uri.as_deref().is_some_and(|u| u.starts_with("battlenet://")));
        }
    }
}
