// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! What the Steam client itself recorded as playtime, read from
//! `<Steam>/userdata/<account>/config/localconfig.vdf`.
//!
//! This is **Steam's figure, not ours**: it covers every machine the account
//! played on, it is as fresh as the last time the client wrote the file, and it
//! is never added to the time Astrail measured. The detail page shows it as its
//! own line.
//!
//! Layout (text VDF, read from a real client file):
//! `UserLocalConfigStore > Software > Valve > Steam > apps > <appid> >
//! { "Playtime" "<minutes>", "LastPlayed" "<unix seconds>" }`. A second `apps`
//! block lives elsewhere in the file with unrelated data, so the whole path is
//! matched, not the key alone. Key case has varied between client versions, so
//! it is compared ignoring ASCII case.
//!
//! Called from the blocking pool only (`lib.rs::steam_playtime`); the parsed
//! table is cached until the file's mtime or length changes.

use serde::Serialize;
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

/// A `localconfig.vdf` is a few hundred KB; anything near this is not one.
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// Path of the per-app blocks below the root key.
const APPS_PATH: [&str; 4] = ["Software", "Valve", "Steam", "apps"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SteamPlaytime {
    /// Minutes, as Steam stores them. Never 0: an unplayed app is reported as absent.
    pub minutes: u32,
    /// Unix seconds of the last launch Steam knows about.
    pub last_played: Option<u64>,
}

struct Cached {
    path: PathBuf,
    modified: SystemTime,
    len: u64,
    apps: HashMap<u32, SteamPlaytime>,
}

static CACHE: Mutex<Option<Cached>> = Mutex::new(None);

/// Steam's own playtime for `app_id`, or `None` when Steam is not installed, no
/// account file exists, the app is not in it, or Steam recorded zero minutes.
pub fn for_app(app_id: u32) -> Option<SteamPlaytime> {
    let steam = steamlocate::SteamDir::locate().ok()?;
    let path = newest_localconfig(&steam.path().join("userdata"))?;
    let meta = std::fs::metadata(&path).ok()?;
    let (modified, len) = (meta.modified().ok()?, meta.len());

    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    let fresh = cache
        .as_ref()
        .is_some_and(|c| c.path == path && c.modified == modified && c.len == len);
    if !fresh {
        if len > MAX_FILE_BYTES {
            log::warn!("steam playtime: localconfig.vdf is {len} bytes, refusing to read it");
            return None;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("steam playtime: could not read localconfig.vdf: {e}");
                return None;
            }
        };
        let apps = parse(&String::from_utf8_lossy(&bytes));
        log::debug!("steam playtime: {} played apps loaded", apps.len());
        *cache = Some(Cached { path, modified, len, apps });
    }
    cache.as_ref()?.apps.get(&app_id).copied()
}

/// The account file written most recently. Several accounts can share a
/// machine; their figures are per account and are never summed, so the one the
/// client touched last (the account in use) is the one reported.
fn newest_localconfig(userdata: &Path) -> Option<PathBuf> {
    std::fs::read_dir(userdata)
        .ok()?
        .flatten()
        .filter_map(|account| {
            let file = account.path().join("config").join("localconfig.vdf");
            let modified = std::fs::metadata(&file).ok()?.modified().ok()?;
            Some((modified, file))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, file)| file)
}

enum Token<'a> {
    Open,
    Close,
    Text(Cow<'a, str>),
}

/// Text-VDF tokens: `{`, `}`, quoted strings (with `\"`, `\\`, `\n`, `\t`
/// escapes), bare words, `//` comments. Never fails: a malformed tail just ends
/// the stream.
struct Lexer<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Iterator for Lexer<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Token<'a>> {
        let bytes = self.src.as_bytes();
        loop {
            while self.pos < bytes.len() && bytes[self.pos].is_ascii_whitespace() {
                self.pos += 1;
            }
            if bytes[self.pos..].starts_with(b"//") {
                while self.pos < bytes.len() && bytes[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }
            break;
        }
        let first = *bytes.get(self.pos)?;
        match first {
            b'{' => {
                self.pos += 1;
                Some(Token::Open)
            }
            b'}' => {
                self.pos += 1;
                Some(Token::Close)
            }
            b'"' => {
                let start = self.pos + 1;
                let mut end = start;
                let mut escaped = false;
                // Every delimiter is ASCII, so byte offsets stay on char boundaries.
                while end < bytes.len() && bytes[end] != b'"' {
                    if bytes[end] == b'\\' && end + 1 < bytes.len() {
                        escaped = true;
                        end += 1;
                    }
                    end += 1;
                }
                let end = end.min(bytes.len());
                self.pos = (end + 1).min(bytes.len());
                let raw = &self.src[start..end];
                Some(Token::Text(if escaped { Cow::Owned(unescape(raw)) } else { Cow::Borrowed(raw) }))
            }
            _ => {
                let start = self.pos;
                while self.pos < bytes.len()
                    && !bytes[self.pos].is_ascii_whitespace()
                    && !matches!(bytes[self.pos], b'{' | b'}' | b'"')
                {
                    self.pos += 1;
                }
                Some(Token::Text(Cow::Borrowed(&self.src[start..self.pos])))
            }
        }
    }
}

fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Every app with a non-zero `Playtime`, keyed by app id.
pub fn parse(text: &str) -> HashMap<u32, SteamPlaytime> {
    let mut apps: HashMap<u32, SteamPlaytime> = HashMap::new();
    // Keys of the blocks we are inside of, root first.
    let mut stack: Vec<Cow<'_, str>> = Vec::new();
    let mut pending: Option<Cow<'_, str>> = None;
    // (minutes, last_played) of the app block being read.
    let mut current: Option<(u32, u32, Option<u64>)> = None;

    for token in (Lexer { src: text, pos: 0 }) {
        match token {
            Token::Open => {
                stack.push(pending.take().unwrap_or_default());
                if stack.len() == APPS_PATH.len() + 2 && inside_apps(&stack) {
                    current = stack.last().and_then(|id| id.parse().ok()).map(|id| (id, 0, None));
                }
            }
            Token::Close => {
                if stack.len() == APPS_PATH.len() + 2 {
                    if let Some((id, minutes, last_played)) = current.take() {
                        if minutes > 0 {
                            apps.insert(id, SteamPlaytime { minutes, last_played });
                        }
                    }
                }
                stack.pop();
                pending = None;
            }
            Token::Text(text) => match pending.take() {
                None => pending = Some(text),
                Some(key) => {
                    // Only the app block's own keys: nested blocks (`cloud`, …) are deeper.
                    if stack.len() != APPS_PATH.len() + 2 {
                        continue;
                    }
                    let Some(app) = current.as_mut() else { continue };
                    if key.eq_ignore_ascii_case("Playtime") {
                        app.1 = text.parse().unwrap_or(0);
                    } else if key.eq_ignore_ascii_case("LastPlayed") {
                        app.2 = text.parse().ok().filter(|ts| *ts > 0);
                    }
                }
            },
        }
    }
    apps
}

/// `stack` = root, then `APPS_PATH`, then the app id. The root key's name is not checked.
fn inside_apps(stack: &[Cow<'_, str>]) -> bool {
    stack[1..=APPS_PATH.len()]
        .iter()
        .zip(APPS_PATH)
        .all(|(have, want)| have.eq_ignore_ascii_case(want))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape copied from a real client file (values changed).
    const SAMPLE: &str = r#"
"UserLocalConfigStore"
{
    "friends"
    {
        "apps"
        {
            "999"
            {
                "Playtime"        "5"
            }
        }
    }
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "apps"
                {
                    "7"
                    {
                        "cloud"
                        {
                            "last_sync_state"        "synchronized"
                        }
                    }
                    "218"
                    {
                        "LastPlayed"        "1619217980"
                        "Playtime"        "377"
                        "cloud"
                        {
                            "Playtime"        "1"
                        }
                    }
                    "440"
                    {
                        "LastPlayed"        "0"
                        "Playtime"        "0"
                    }
                }
            }
        }
    }
    "apps"
    {
        "431960"
        {
            "Playtime"        "9"
        }
    }
}
"#;

    #[test]
    fn only_the_steam_apps_block_is_read() {
        let apps = parse(SAMPLE);
        assert_eq!(
            apps.get(&218),
            Some(&SteamPlaytime { minutes: 377, last_played: Some(1_619_217_980) })
        );
        // Same key names elsewhere in the file are somebody else's data.
        assert!(!apps.contains_key(&999));
        assert!(!apps.contains_key(&431_960));
        assert_eq!(apps.len(), 1);
    }

    #[test]
    fn a_nested_block_cannot_overwrite_the_apps_own_keys() {
        assert_eq!(parse(SAMPLE)[&218].minutes, 377);
    }

    #[test]
    fn an_unplayed_app_is_absent_rather_than_zero() {
        let apps = parse(SAMPLE);
        assert!(!apps.contains_key(&440));
        assert!(!apps.contains_key(&7));
    }

    #[test]
    fn key_case_is_ignored() {
        let text = r#""UserLocalConfigStore" { "software" { "valve" { "steam" { "Apps" { "10" { "playtime" "42" } } } } } }"#;
        assert_eq!(parse(text)[&10], SteamPlaytime { minutes: 42, last_played: None });
    }

    #[test]
    fn escaped_quotes_and_comments_do_not_derail_the_path() {
        let text = concat!(
            "\"Root\"\n{\n",
            "\t// a comment with \"quotes\" and { braces }\n",
            "\t\"layout\"\t\t\"{\\\"currentLayout\\\":0,\\\"x\\\":\\\"}\\\"}\"\n",
            "\t\"Software\" { \"Valve\" { \"Steam\" { \"apps\" { \"20\" { \"Playtime\" \"3\" } } } } }\n",
            "}\n",
        );
        assert_eq!(parse(text)[&20].minutes, 3);
    }

    #[test]
    fn garbage_and_truncated_input_never_panic() {
        assert!(parse("").is_empty());
        assert!(parse("}}}}{{{{").is_empty());
        assert!(parse("\"unterminated").is_empty());
        assert!(parse("\"a\" { \"b\" \"trailing backslash \\").is_empty());
        let cut = &SAMPLE[..SAMPLE.len() / 2];
        let _ = parse(cut);
        // Non-numeric ids and values are skipped, not fatal.
        let text = r#""R" { "Software" { "Valve" { "Steam" { "apps" { "abc" { "Playtime" "9" } "30" { "Playtime" "lots" } } } } } }"#;
        assert!(parse(text).is_empty());
    }

    /// Needs a Steam client with a logged-in account: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn the_real_client_file_parses() {
        let steam = steamlocate::SteamDir::locate().expect("Steam is installed");
        let path = newest_localconfig(&steam.path().join("userdata")).expect("an account file");
        let apps = parse(&String::from_utf8_lossy(&std::fs::read(path).expect("readable")));
        println!("{} played apps", apps.len());
        assert!(!apps.is_empty());
        assert!(apps.values().all(|a| a.minutes > 0));
    }

    #[test]
    fn unescape_handles_the_vdf_escapes() {
        assert_eq!(unescape(r#"a\"b\\c\nd\te"#), "a\"b\\c\nd\te");
        assert_eq!(unescape("tail\\"), "tail\\");
    }
}
