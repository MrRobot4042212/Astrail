// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

use crate::models::Game;

/// Scan installed programs from the Windows uninstall registry as a catch-all
/// for games from launchers we don't parse natively (Battle.net, Riot, Rockstar,
/// Amazon, itch, standalone installers…).
///
/// This is inherently noisy — the registry lists every program, not just games —
/// so we filter aggressively (skip system components, runtimes, drivers, the
/// launcher clients themselves…). False positives can be hidden from the UI.
/// Entries from known launchers are tagged with a specific `GameSource`; the
/// rest fall back to `GameSource::Windows`.
#[cfg(windows)]
pub fn scan() -> Result<Vec<Game>, String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    const ROOTS: &[(isize, &str)] = &[
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_CURRENT_USER, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    ];

    let mut games = Vec::new();
    for (hive, path) in ROOTS {
        let root = match RegKey::predef(*hive).open_subkey(path) {
            Ok(k) => k,
            Err(_) => continue,
        };
        for sub in root.enum_keys().flatten() {
            let Ok(entry) = root.open_subkey(&sub) else {
                continue;
            };
            if let Some(game) = from_entry(&entry, &sub) {
                games.push(game);
            }
        }
    }

    Ok(games)
}

#[cfg(windows)]
fn from_entry(entry: &winreg::RegKey, key_name: &str) -> Option<Game> {
    let raw: String = entry.get_value("DisplayName").ok()?;
    let raw = raw.trim();
    // Show a clean product name: many entries append version/arch (e.g. "OBS
    // Studio 30.0.2", "7-Zip (x64)", "Git version 2.43.0").
    let cleaned = clean_name(raw);
    let name = if cleaned.is_empty() { raw.to_string() } else { cleaned };
    if name.is_empty() {
        return None;
    }

    // Skip OS/update components and sub-entries of other installs.
    if entry.get_value::<u32, _>("SystemComponent").unwrap_or(0) == 1 {
        return None;
    }
    if entry.get_value::<String, _>("ParentKeyName").map(|s| !s.is_empty()).unwrap_or(false) {
        return None;
    }
    if let Ok(release) = entry.get_value::<String, _>("ReleaseType") {
        let r = release.to_lowercase();
        if r.contains("update") || r.contains("hotfix") {
            return None;
        }
    }

    let publisher: String = entry.get_value("Publisher").unwrap_or_default();
    let install_location: String = entry.get_value("InstallLocation").unwrap_or_default();
    let display_icon: String = entry.get_value("DisplayIcon").unwrap_or_default();

    if is_junk(&name, &publisher) {
        return None;
    }

    let executable = exe_from_icon(&display_icon).or_else(|| find_launch_exe(&install_location))?;

    let source = classify(&name, &publisher, &display_icon, &install_location);

    Some(Game {
        // Stable id from the uninstall registry key (a GUID/product code or the
        // installer's own key) — survives DisplayName changes/cleaning, so user
        // overlays (hidden/favorites/categories/playtime) don't get lost.
        id: format!("windows:{}", key_name.to_lowercase()),
        name,
        source,
        app_id: None,
        executable: Some(executable),
        install_dir: (!install_location.trim().is_empty()).then_some(install_location),
        cover_url: None,
        launch_uri: None,
        favorite: false,
        categories: Vec::new(),

    })
}

/// Decide the source for a registry entry: a known game launcher, an
/// auto-detected application, or the generic (likely-game) catch-all.
#[cfg(windows)]
fn classify(
    name: &str,
    publisher: &str,
    display_icon: &str,
    install_location: &str,
) -> crate::models::GameSource {
    use crate::models::GameSource;
    let hay = format!("{publisher} {display_icon} {install_location}").to_lowercase();
    if hay.contains("blizzard") || hay.contains("battle.net") {
        GameSource::Battlenet
    } else if hay.contains("riot games") || hay.contains("riot client") {
        GameSource::Riot
    } else if hay.contains("rockstar") {
        GameSource::Rockstar
    } else if hay.contains("amazon games") || hay.contains(r"amazon games\library") {
        GameSource::Amazon
    } else if hay.contains("battlestate games") || hay.contains("escape from tarkov") || hay.contains("bsglauncher") {
        GameSource::Battlestate
    } else if crate::apps_db::is_app(name, publisher, install_location) {
        GameSource::App
    } else {
        GameSource::Windows
    }
}

/// True for entries that are never launchable library items: runtimes, drivers,
/// system tooling, hardware-vendor software and the store launcher clients
/// themselves (we want their games, not the client entry). Apps the user might
/// actually want (browsers, office, dev tools…) are NOT dropped here — they are
/// surfaced and tagged `GameSource::App` by `classify`/`is_app`.
///
/// Matching is deliberately tiered. A single substring blocklist silently ate real
/// titles — "SteamWorld Dig 2" ("steam"), "Assassin's Creed Origins" ("origin"),
/// "Driver: San Francisco" ("driver") — and a missing game is an invisible failure,
/// while a surviving junk entry is one the user can hide in two clicks. The bias is
/// therefore towards keeping.
#[cfg(windows)]
fn is_junk(name: &str, publisher: &str) -> bool {
    let n = name.trim().to_lowercase();
    let p = publisher.to_lowercase();

    // Tier 1 — matched against the WHOLE cleaned name. Launcher clients (we want
    // their games, not the client entry) and apps deliberately kept out of the
    // library. `clean_name` has already stripped version/arch tails, so "7-Zip
    // 24.09 (x64)" arrives here as "7-zip".
    const EXACT_BLOCK: &[&str] = &[
        "steam", "steam client", "epic games launcher", "gog galaxy", "ea app",
        "ea desktop", "origin", "ubisoft connect", "uplay", "battle.net",
        "riot client", "rockstar games launcher", "amazon games", "discord",
        "overwolf", "playnite", "geforce now", "meteor", "blender", "7-zip",
        "wallpaper engine", "youtube",
    ];
    if EXACT_BLOCK.contains(&n.as_str()) {
        return true;
    }

    // Tier 2 — fragments that do not occur in the name of anything launchable.
    const FRAGMENT_BLOCK: &[&str] = &[
        "redistributable", "redist", "vcredist", "visual c++", "directx", "vulkan",
        "webview2", "microsoft edge", "onedrive", "dotnet", ".net framework",
        ".net runtime", ".net core", ".net sdk", "java(tm)", "java se",
        "windows software", "debugging tools", "maintenance service",
        "update health", "google update", "active directory", "service pack",
        "hotfix", "update for", "geforce experience", "radeon software",
        "python launcher",
    ];
    if FRAGMENT_BLOCK.iter().any(|b| n.contains(b)) {
        return true;
    }

    // Tier 3 — generic words that DO appear in real titles, so they only count on a
    // short vendor-style name of at most two words. "Driver: San Francisco" and
    // "Setup Wizard Deluxe Adventure" survive; "Audio Driver" and "Java Runtime"
    // do not. Longer vendor entries are caught by the publisher list below.
    const SHORT_NAME_WORD_BLOCK: &[&str] = &[
        "driver", "drivers", "setup", "installer", "uninstall", "runtime",
        "framework", "sdk", "python", "java", "nvidia", "geforce", "radeon",
    ];
    if n.split_whitespace().count() <= 2
        && SHORT_NAME_WORD_BLOCK.iter().any(|b| contains_word(&n, b))
    {
        return true;
    }

    // Hardware/driver and pure-runtime vendors only — consumer-app publishers
    // (Microsoft, Google, Adobe, Mozilla…) are handled as apps, not blocked.
    const PUB_BLOCK: &[&str] = &[
        "nvidia", "advanced micro devices", "intel", "realtek", "oracle",
        "python software foundation",
    ];
    PUB_BLOCK.iter().any(|b| p.contains(b))
}

/// `haystack.contains(word)` restricted to alphanumeric boundaries, so "driver"
/// matches "audio driver" but not "drivereasy", and "steam" does not match
/// "steamworld". Both arguments are expected to be lowercased.
#[cfg(windows)]
fn contains_word(haystack: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(hit) = haystack[from..].find(word) {
        let start = from + hit;
        let end = start + word.len();
        let before_ok = !haystack[..start].chars().next_back().is_some_and(char::is_alphanumeric);
        let after_ok = !haystack[end..].chars().next().is_some_and(char::is_alphanumeric);
        if before_ok && after_ok {
            return true;
        }
        from = end;
    }
    false
}

/// Strip version/architecture noise from a registry `DisplayName`, leaving the
/// plain product name (e.g. "OBS Studio 30.0.2" → "OBS Studio", "7-Zip (x64)" →
/// "7-Zip", "Git version 2.43.0" → "Git"). Conservative: keeps meaningful numbers
/// without a dot (editions/years like "Office 365", "Visual Studio 2022").
#[cfg(windows)]
fn clean_name(raw: &str) -> String {
    strip_version_tail(&strip_noise_brackets(raw))
}

/// Drop bracketed chunks `(...)`/`[...]` that are version/arch noise (contain a
/// digit or "bit"/"x64"/"x86"); keep purely descriptive ones.
#[cfg(windows)]
fn strip_noise_brackets(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        let close = match c {
            '(' => Some(')'),
            '[' => Some(']'),
            _ => None,
        };
        let Some(close) = close else {
            out.push(c);
            continue;
        };
        let mut inner = String::new();
        let mut closed = false;
        for ic in chars.by_ref() {
            if ic == close {
                closed = true;
                break;
            }
            inner.push(ic);
        }
        let low = inner.to_lowercase();
        let noise = low.chars().any(|c| c.is_ascii_digit())
            || low.contains("bit")
            || low.contains("x64")
            || low.contains("x86");
        if !closed {
            // Unmatched bracket: keep the text as-is, don't lose content.
            out.push(c);
            out.push_str(&inner);
        } else if !noise {
            out.push(c);
            out.push_str(&inner);
            out.push(close);
        }
        // else: drop the noisy bracket entirely
    }
    out
}

/// Drop trailing version/arch tokens ("2.43.0", "v1.2", "x64", "64-bit",
/// "version"), stopping at the first token that's part of the real name.
#[cfg(windows)]
fn strip_version_tail(s: &str) -> String {
    let mut tokens: Vec<&str> = s.split_whitespace().collect();
    while let Some(last) = tokens.last() {
        let low = last.to_lowercase();
        let no_v = last.trim_start_matches(['v', 'V']);
        let dotted_version = !no_v.is_empty()
            && no_v.contains('.')
            && no_v.chars().all(|c| c.is_ascii_digit() || c == '.');
        let arch = matches!(
            low.as_str(),
            "x64" | "x86" | "64-bit" | "32-bit" | "win64" | "win32" | "amd64" | "(x64)" | "(x86)"
        );
        if dotted_version || arch || low == "version" {
            tokens.pop();
        } else {
            break;
        }
    }
    tokens
        .join(" ")
        .trim_end_matches([' ', '-', ',', ':', '·'])
        .trim()
        .to_string()
}

/// Extract an `.exe` path from a `DisplayIcon` value (`C:\game\g.exe,0`).
#[cfg(windows)]
fn exe_from_icon(display_icon: &str) -> Option<String> {
    let path = display_icon.split(',').next().unwrap_or("").trim().trim_matches('"');
    if path.to_lowercase().ends_with(".exe") && std::path::Path::new(path).is_file() {
        Some(path.to_string())
    } else {
        None
    }
}

/// Pick a plausible game executable from `dir`, skipping helpers/installers.
#[cfg(windows)]
fn find_launch_exe(dir: &str) -> Option<String> {
    if dir.trim().is_empty() {
        return None;
    }
    let skip = ["unins", "setup", "vcredist", "redist", "crash", "launcher_installer", "dxsetup"];
    let mut best: Option<std::path::PathBuf> = None;
    let mut best_size = 0u64;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("exe")) != Some(true) {
            continue;
        }
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        if skip.iter().any(|s| stem.contains(s)) {
            continue;
        }
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


#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn strips_version_and_arch_tails() {
        assert_eq!(clean_name("OBS Studio 30.0.2"), "OBS Studio");
        assert_eq!(clean_name("Git version 2.43.0"), "Git");
        assert_eq!(clean_name("Foo v1.2"), "Foo");
        assert_eq!(clean_name("Foo x64"), "Foo");
        assert_eq!(clean_name("Foo 64-bit"), "Foo");
        // Trailing separators left behind by the strip are trimmed too.
        assert_eq!(clean_name("Foo - v1.0"), "Foo");
    }

    #[test]
    fn keeps_meaningful_numbers_without_a_dot() {
        assert_eq!(clean_name("Office 365"), "Office 365");
        assert_eq!(clean_name("Visual Studio 2022"), "Visual Studio 2022");
        assert_eq!(clean_name("Half-Life 2"), "Half-Life 2");
    }

    #[test]
    fn drops_noisy_brackets_but_keeps_descriptive_ones() {
        assert_eq!(clean_name("7-Zip (x64)"), "7-Zip");
        assert_eq!(clean_name("Foo (64-bit)"), "Foo");
        assert_eq!(clean_name("Foo [1.2.3]"), "Foo");
        assert_eq!(clean_name("Foo (Beta)"), "Foo (Beta)");
    }

    #[test]
    fn an_unmatched_bracket_keeps_its_text_instead_of_losing_it() {
        assert_eq!(clean_name("Foo (x64"), "Foo (x64");
    }

    #[test]
    fn blocks_runtimes_drivers_and_system_tooling_by_name() {
        assert!(is_junk("Microsoft Visual C++ 2015 Redistributable", ""));
        assert!(is_junk("NVIDIA GeForce Experience", ""));
        assert!(is_junk("Java(TM) SE Development Kit", ""));
        assert!(is_junk("Windows Software Development Kit", ""));
        assert!(is_junk("Microsoft Edge WebView2 Runtime", ""));
    }

    #[test]
    fn blocks_store_launcher_clients_so_only_their_games_remain() {
        assert!(is_junk("Steam", ""));
        assert!(is_junk("Ubisoft Connect", ""));
        assert!(is_junk("EA app", ""));
        assert!(is_junk("Battle.net", ""));
        assert!(is_junk("GOG Galaxy", ""));
    }

    #[test]
    fn blocks_hardware_and_runtime_vendors_by_publisher() {
        assert!(is_junk("Some Control Panel", "NVIDIA Corporation"));
        assert!(is_junk("Some Audio Tool", "Realtek Semiconductor Corp."));
        assert!(is_junk("Some Utility", "Advanced Micro Devices, Inc."));
        assert!(is_junk("Some Tool", "Python Software Foundation"));
    }

    #[test]
    fn keeps_consumer_apps_and_ordinary_games() {
        // Consumer-app publishers are deliberately not blocked; `classify` tags
        // these as GameSource::App instead of dropping them.
        assert!(!is_junk("Google Chrome", "Google LLC"));
        assert!(!is_junk("Adobe Photoshop", "Adobe Inc."));
        assert!(!is_junk("Hollow Knight", "Team Cherry"));
        assert!(!is_junk("Cyberpunk 2077", "CD PROJEKT RED"));
    }

    // --- Regression tests for the substring-blocklist defect fixed on 2026-09-08. ---

    #[test]
    fn real_titles_containing_a_blocked_word_survive() {
        // Every one of these was dropped by the old plain-substring blocklist. A
        // missing game is an invisible failure; a junk entry that survives is one
        // click to hide, so the matching is biased towards keeping.
        assert!(!is_junk("Driver: San Francisco", "Ubisoft"));
        assert!(!is_junk("SteamWorld Dig 2", "Image & Form"));
        assert!(!is_junk("Assassin's Creed Origins", "Ubisoft"));
        assert!(!is_junk("Discordia", "Indie Dev"));
        assert!(!is_junk("Deus Ex: Human Revolution", "Eidos"));
    }

    #[test]
    fn short_vendor_style_names_are_still_blocked_by_a_generic_word() {
        // The same words stay lethal on a one- or two-word name, which is what
        // registry junk actually looks like.
        assert!(is_junk("Audio Driver", ""));
        assert!(is_junk("Realtek Drivers", ""));
        assert!(is_junk("Java Runtime", ""));
        assert!(is_junk("Python 3.12", ""));
    }

    #[test]
    fn word_matching_respects_alphanumeric_boundaries() {
        assert!(contains_word("audio driver", "driver"));
        assert!(contains_word("driver: san francisco", "driver"));
        assert!(!contains_word("drivereasy", "driver"));
        assert!(!contains_word("steamworld", "steam"));
        assert!(!contains_word("", "steam"));
        assert!(!contains_word("steam", ""));
    }
}
