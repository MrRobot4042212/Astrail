//! Pure steps of library assembly: duplicate merging and the user overlays
//! (favorites, categories, cover and type overrides) that `get_library` applies
//! on top of what the scanners found.
//!
//! Scanners emit one entry per store, so a game owned in two stores — or seen
//! both by its store scanner and by the generic registry scan — appears twice
//! under the same name. Only the highest-priority copy is shown, but every user
//! overlay is keyed by **id**, and the dropped copy's id is exactly the one the
//! user may have favorited or re-covered before the second store appeared (or
//! after it went away). `merge_duplicates` therefore keeps an alias map from
//! each dropped id to the id that survived, and `apply_overlays` honours the
//! overlays stored under either.
//!
//! Hidden ids are deliberately *not* aliased: hiding is the user's answer to a
//! specific entry, and propagating it to a store copy that appears later would
//! make that copy impossible to unhide from the library view.

use crate::models::{Game, GameSource};
use std::collections::{HashMap, HashSet};

/// Result of `merge_duplicates`.
pub struct Merged {
    pub games: Vec<Game>,
    /// `dropped id -> kept id` for every duplicate that was removed.
    pub aliases: HashMap<String, String>,
}

/// Collapse entries that share a name (case-insensitively), keeping the first —
/// i.e. highest-priority — occurrence, and record which ids were folded into it.
///
/// Manually added entries are never dropped and never claim a name: the user
/// created them on purpose, typically to launch a different executable than the
/// store copy, so both must stay.
pub fn merge_duplicates(games: Vec<Game>) -> Merged {
    let mut kept_by_name: HashMap<String, String> = HashMap::new();
    let mut aliases = HashMap::new();
    let mut out = Vec::with_capacity(games.len());
    for game in games {
        if game.source == GameSource::Manual {
            out.push(game);
            continue;
        }
        let key = game.name.to_lowercase();
        match kept_by_name.get(&key) {
            Some(kept) => {
                aliases.insert(game.id, kept.clone());
            }
            None => {
                kept_by_name.insert(key, game.id.clone());
                out.push(game);
            }
        }
    }
    Merged { games: out, aliases }
}

/// Everything the user layered on top of the scanned library, keyed by game id.
#[derive(Default)]
pub struct Overlays {
    pub favorites: Vec<String>,
    pub categories: HashMap<String, Vec<String>>,
    pub covers: HashMap<String, String>,
    /// `"app"` / `"game"` per id (any other value is ignored).
    pub types: HashMap<String, String>,
}

/// Apply the overlays to `games`, resolving each through `aliases` so a value
/// stored under a folded duplicate's id still lands on the surviving entry. An
/// entry's own id always wins over its aliases; among aliases, the first one in
/// scan order wins.
pub fn apply_overlays(games: &mut [Game], aliases: &HashMap<String, String>, overlays: &Overlays) {
    if overlays.favorites.is_empty()
        && overlays.categories.is_empty()
        && overlays.covers.is_empty()
        && overlays.types.is_empty()
    {
        return;
    }

    // kept id -> ids folded into it. A HashMap's iteration order is not stable,
    // so sort the aliases to make "first alias wins" deterministic.
    let mut folded: HashMap<&str, Vec<&str>> = HashMap::new();
    for (dropped, kept) in aliases {
        folded.entry(kept.as_str()).or_default().push(dropped.as_str());
    }
    for ids in folded.values_mut() {
        ids.sort_unstable();
    }
    let empty: Vec<&str> = Vec::new();

    let favorites: HashSet<&str> = overlays
        .favorites
        .iter()
        .map(|id| aliases.get(id).unwrap_or(id).as_str())
        .collect();

    for game in games.iter_mut() {
        let own = game.id.as_str();
        let others = folded.get(own).unwrap_or(&empty);
        let lookup = |map: &HashMap<String, String>| -> Option<String> {
            map.get(own)
                .or_else(|| others.iter().find_map(|id| map.get(*id)))
                .cloned()
        };

        if favorites.contains(own) {
            game.favorite = true;
        }

        // Categories are a union: the user may have tagged either copy.
        let mut cats: Vec<String> = Vec::new();
        for id in std::iter::once(own).chain(others.iter().copied()) {
            if let Some(list) = overlays.categories.get(id) {
                for cat in list {
                    if !cats.contains(cat) {
                        cats.push(cat.clone());
                    }
                }
            }
        }
        if !cats.is_empty() {
            game.categories = cats;
        }

        if let Some(url) = lookup(&overlays.covers) {
            game.cover_url = Some(url);
        }

        // "game" only needs to undo an App detection (store sources are already
        // games), so the real source is kept whenever possible and removing the
        // override self-heals on the next scan.
        match lookup(&overlays.types).as_deref() {
            Some("app") => game.source = GameSource::App,
            Some("game") if game.source == GameSource::App => {
                game.source = GameSource::Windows;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str, name: &str, source: GameSource) -> Game {
        Game {
            id: id.to_string(),
            name: name.to_string(),
            source,
            app_id: None,
            executable: None,
            install_dir: None,
            cover_url: None,
            launch_uri: None,
            favorite: false,
            categories: Vec::new(),
        }
    }

    fn ids(games: &[Game]) -> Vec<&str> {
        games.iter().map(|g| g.id.as_str()).collect()
    }

    #[test]
    fn the_first_copy_wins_and_the_dropped_id_becomes_an_alias() {
        let merged = merge_duplicates(vec![
            game("steam:1", "Hades", GameSource::Steam),
            game("epic:hades", "HADES", GameSource::Epic),
            game("windows:hades", "hades", GameSource::Windows),
            game("gog:2", "Celeste", GameSource::Gog),
        ]);
        assert_eq!(ids(&merged.games), ["steam:1", "gog:2"]);
        assert_eq!(merged.aliases.get("epic:hades").map(String::as_str), Some("steam:1"));
        assert_eq!(merged.aliases.get("windows:hades").map(String::as_str), Some("steam:1"));
        assert_eq!(merged.aliases.len(), 2);
    }

    #[test]
    fn a_manual_entry_survives_a_scanned_entry_with_the_same_name() {
        // Regression (H4): manual apps were loaded last and the name dedup
        // silently dropped any whose name matched a scanned entry.
        let merged = merge_duplicates(vec![
            game("steam:1", "Hades", GameSource::Steam),
            game("manual:1", "Hades", GameSource::Manual),
            game("manual:2", "Hades", GameSource::Manual),
        ]);
        assert_eq!(ids(&merged.games), ["steam:1", "manual:1", "manual:2"]);
        assert!(merged.aliases.is_empty());
    }

    #[test]
    fn overlays_stored_under_a_folded_id_land_on_the_kept_entry() {
        // Regression (H4): the user favorited / re-covered the registry copy;
        // installing the Steam copy changed the shown id and every overlay
        // silently stopped applying.
        let merged = merge_duplicates(vec![
            game("steam:1", "Hades", GameSource::Steam),
            game("windows:hades", "Hades", GameSource::Windows),
            game("windows:tool", "Some Tool", GameSource::App),
        ]);
        let mut games = merged.games;
        let overlays = Overlays {
            favorites: vec!["windows:hades".into()],
            categories: HashMap::from([
                ("windows:hades".to_string(), vec!["Roguelike".to_string()]),
                ("steam:1".to_string(), vec!["Indie".to_string(), "Roguelike".to_string()]),
            ]),
            covers: HashMap::from([("windows:hades".to_string(), "C:\\c\\hades.jpg".to_string())]),
            types: HashMap::from([("windows:tool".to_string(), "game".to_string())]),
        };
        apply_overlays(&mut games, &merged.aliases, &overlays);

        let hades = &games[0];
        assert!(hades.favorite);
        assert_eq!(hades.categories, ["Indie", "Roguelike"]);
        assert_eq!(hades.cover_url.as_deref(), Some("C:\\c\\hades.jpg"));
        assert_eq!(games[1].source, GameSource::Windows);
    }

    #[test]
    fn an_entrys_own_overlay_beats_its_aliases() {
        let merged = merge_duplicates(vec![
            game("steam:1", "Hades", GameSource::Steam),
            game("windows:hades", "Hades", GameSource::Windows),
        ]);
        let mut games = merged.games;
        let overlays = Overlays {
            covers: HashMap::from([
                ("steam:1".to_string(), "own.jpg".to_string()),
                ("windows:hades".to_string(), "alias.jpg".to_string()),
            ]),
            ..Overlays::default()
        };
        apply_overlays(&mut games, &merged.aliases, &overlays);
        assert_eq!(games[0].cover_url.as_deref(), Some("own.jpg"));
    }
}
