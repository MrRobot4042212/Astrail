// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

use serde::Deserialize;
use std::sync::{Condvar, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// IGDB (Twitch) app credentials. **Build-time environment only**: they are read
// from `IGDB_CLIENT_ID` / `IGDB_CLIENT_SECRET` when the binary is compiled (CI
// secrets, or a gitignored `.env` locally — see `.env.example`).
//
// There is deliberately **no fallback constant**: an embedded client secret is
// extractable from any shipped binary, so a build without the variables simply
// ships without IGDB cover lookups (`resolve_cover` returns `None` and logs
// once) instead of leaking one shared credential to every user.
const CLIENT_ID: Option<&str> = option_env!("IGDB_CLIENT_ID");
const CLIENT_SECRET: Option<&str> = option_env!("IGDB_CLIENT_SECRET");

/// Warn once (not per lookup) when the build has no IGDB credentials.
static MISSING_LOGGED: OnceLock<()> = OnceLock::new();

/// The build-time credentials, or `None` (logged once) when this binary was
/// built without them.
fn credentials() -> Option<(&'static str, &'static str)> {
    match (CLIENT_ID, CLIENT_SECRET) {
        (Some(id), Some(secret)) if !id.trim().is_empty() && !secret.trim().is_empty() => {
            Some((id, secret))
        }
        _ => {
            MISSING_LOGGED.get_or_init(|| {
                eprintln!(
                    "[igdb] no IGDB credentials in this build (IGDB_CLIENT_ID / IGDB_CLIENT_SECRET); cover lookups are disabled"
                );
            });
            None
        }
    }
}

/// Cached Twitch app access token (IGDB auths through Twitch). Tokens last ~60
/// days; we cache it process-wide and refresh shortly before expiry.
struct Token {
    value: String,
    expires_at: u64,
}
static TOKEN: Mutex<Option<Token>> = Mutex::new(None);

/// Process-wide HTTP agent: reuses the TCP connection pool across all IGDB cover
/// lookups so each call doesn't pay the TLS/TCP handshake cost from scratch.
static AGENT: OnceLock<ureq::Agent> = OnceLock::new();

fn agent() -> &'static ureq::Agent {
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(6))
            .timeout_read(Duration::from_secs(8))
            .build()
    })
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// How long to stop hammering Twitch after a failed token request. Without it,
/// every pending cover lookup retries the fetch, each paying the full connect
/// timeout, whenever the network is down.
const TOKEN_RETRY_BACKOFF: Duration = Duration::from_secs(30);
static TOKEN_RETRY_AFTER: Mutex<Option<Instant>> = Mutex::new(None);

/// Get a valid Twitch app token, reusing the cached one when possible.
///
/// The HTTP request deliberately happens **outside** the `TOKEN` lock. Holding it
/// across the call turned the mutex into a serializer on the failure path: with
/// Twitch unreachable, N concurrent cover lookups queued up and each paid its own
/// 6 s connect timeout in turn, one after another, every one of them occupying an
/// async-runtime worker while it waited.
fn token() -> Option<String> {
    let (client_id, client_secret) = credentials()?;

    {
        let guard = TOKEN.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(tok) = guard.as_ref() {
            // 60s safety margin so a token doesn't expire mid-request.
            if now() + 60 < tok.expires_at {
                return Some(tok.value.clone());
            }
        }
    }

    {
        let mut retry = TOKEN_RETRY_AFTER
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match *retry {
            Some(at) if Instant::now() < at => return None,
            // The window elapsed: clear it so exactly this caller retries.
            Some(_) => *retry = None,
            None => {}
        }
    }

    #[derive(Deserialize)]
    struct TokenResp {
        access_token: String,
        expires_in: u64,
    }

    let fetched: Option<TokenResp> = agent()
        .post("https://id.twitch.tv/oauth2/token")
        .query("client_id", client_id)
        .query("client_secret", client_secret)
        .query("grant_type", "client_credentials")
        .call()
        .ok()
        .and_then(|r| r.into_json().ok());

    let Some(resp) = fetched else {
        *TOKEN_RETRY_AFTER
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Instant::now() + TOKEN_RETRY_BACKOFF);
        eprintln!("[igdb] token request failed; pausing cover lookups for {TOKEN_RETRY_BACKOFF:?}");
        return None;
    };

    let value = resp.access_token;
    *TOKEN.lock().unwrap_or_else(PoisonError::into_inner) = Some(Token {
        value: value.clone(),
        expires_at: now() + resp.expires_in,
    });
    Some(value)
}

/// Maximum IGDB lookups allowed in flight at once.
///
/// The only bound used to be the frontend's worker pool. Nothing in Rust stopped
/// a second window, a future caller or two overlapping scans from firing hundreds
/// of concurrent lookups, each holding a blocking-pool thread for up to a 6 s
/// connect plus an 8 s read, times three name variants. A limit that protects the
/// process belongs in the process.
const MAX_INFLIGHT: usize = 4;
static INFLIGHT: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());

/// RAII permit: releases its slot however the lookup ends, early return included.
struct Permit;

impl Drop for Permit {
    fn drop(&mut self) {
        let (lock, cv) = &INFLIGHT;
        let mut n = lock.lock().unwrap_or_else(PoisonError::into_inner);
        *n = n.saturating_sub(1);
        cv.notify_one();
    }
}

/// Block until a slot is free. Callers already run on the blocking pool, which is
/// exactly where waiting is allowed.
fn acquire_permit() -> Permit {
    let (lock, cv) = &INFLIGHT;
    let mut n = lock.lock().unwrap_or_else(PoisonError::into_inner);
    while *n >= MAX_INFLIGHT {
        n = cv
            .wait(n)
            .unwrap_or_else(|e| e.into_inner());
    }
    *n += 1;
    Permit
}

/// Outcome of a cover lookup.
///
/// "IGDB has nothing for this game" and "we could not ask IGDB" are different
/// facts with different lifetimes: the first is worth remembering for days, the
/// second must never be written to the cache at all. Collapsing both into an
/// empty string is what let a single offline library scan poison every cover for
/// the full negative TTL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// IGDB answered with cover art. Carries the **image id**, not a URL: the
    /// caller composes the URL for the size variant it wants.
    Found(String),
    /// IGDB answered and has no cover for any of the name variants.
    NotFound,
    /// The lookup could not be performed (no credentials in this build, no token,
    /// network failure). Nothing may be cached from this.
    Unavailable,
}

/// Resolve a vertical cover via IGDB for any of the given name variants.
///
/// Returns the IGDB **image id**. It used to return a fully composed
/// `t_cover_big_2x` URL, which the caller then stored in its `image_id` field and
/// fed back into its own URL builder — composing a URL *inside* a URL, so every
/// freshly resolved cover produced a malformed link (and the requested size
/// variant was ignored). Handing back the id leaves URL construction in the one
/// place that knows which variant it wants.
pub fn resolve_cover(variants: &[String]) -> Lookup {
    #[derive(Deserialize)]
    struct Game {
        #[serde(default)]
        name: String,
        cover: Option<Cover>,
    }
    #[derive(Deserialize)]
    struct Cover {
        image_id: String,
    }

    let Some((client_id, _)) = credentials() else {
        return Lookup::Unavailable;
    };
    let Some(token) = token() else {
        return Lookup::Unavailable;
    };
    // Held for the whole lookup, including every name variant.
    let _permit = acquire_permit();
    let agent = agent();
    let bearer = format!("Bearer {token}");

    // Set when a request fails rather than merely coming back empty, so the caller
    // is told "could not ask" instead of "no cover exists".
    let mut transport_failed = false;

    for variant in variants {
        // Apicalypse query: search by name, only games that have cover art.
        let escaped = variant.replace('\\', "\\\\").replace('"', "\\\"");
        let body =
            format!("search \"{escaped}\"; fields name,cover.image_id; where cover != null; limit 6;");

        let Some(games): Option<Vec<Game>> = agent
            .post("https://api.igdb.com/v4/games")
            .set("Client-ID", client_id)
            .set("Authorization", &bearer)
            .set("Accept", "application/json")
            .send_string(&body)
            .ok()
            .and_then(|r| r.into_json().ok())
        else {
            transport_failed = true;
            continue;
        };
        if games.is_empty() {
            continue;
        }

        // Prefer an exact (case-insensitive) name match, else the first hit.
        let chosen = games
            .iter()
            .find(|g| g.name.eq_ignore_ascii_case(variant) && g.cover.is_some())
            .or_else(|| games.iter().find(|g| g.cover.is_some()));
        let Some(image_id) = chosen.and_then(|g| g.cover.as_ref()).map(|c| &c.image_id) else {
            continue;
        };
        return Lookup::Found(image_id.clone());
    }

    // Every variant came back empty. Only call that a real miss when no request
    // failed along the way.
    if transport_failed {
        Lookup::Unavailable
    } else {
        Lookup::NotFound
    }
}
