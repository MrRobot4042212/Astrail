// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Discord Rich Presence: shows the game you're playing in your Discord status.
//!
//! Connects to the local Discord IPC pipe (pure Rust, no Discord SDK). Driven by
//! the playtime watcher: when a library game starts running it sets the activity
//! (`details` = game name, `state` = "Jugando", elapsed timer from the session
//! start); when nothing is running it clears it.
//!
//! Requires a **Discord application client id** (created at
//! <https://discord.com/developers/applications>) — set it in Ajustes. Discord
//! shows "Playing <your app's name>" so naming that app "Astrail" reads best.
//! Everything is best-effort: if Discord isn't running or no id is set, it no-ops.

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Whether the user opted in to Rich Presence (`AppSettings.discord_enabled`).
///
/// Off until the stored settings say otherwise: publishing the game you are
/// playing to your entire friends list is not something to do without being
/// asked. Kept as an atomic so the playtime watcher can check it per tick without
/// reading the settings file.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether Rich Presence is currently allowed to publish anything.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Apply the user's preference. Turning it off drops the presence immediately
/// rather than waiting for the watcher's next tick.
pub fn set_enabled(value: bool) {
    let was = ENABLED.swap(value, Ordering::Relaxed);
    if was && !value {
        clear();
    }
}

/// Optional asset key uploaded to the Discord app's Rich Presence art; empty =
/// no image shown.
const LARGE_IMAGE: &str = "";

/// Built-in Discord **Application ID** so every user gets Rich Presence with zero
/// setup. The Application ID is **not a secret** (only the client *secret* is, and
/// RPC doesn't use it), so it's safe to embed — same approach as the IGDB creds.
/// Create one app at <https://discord.com/developers/applications> (name it
/// "Astrail"), then either paste its numeric id below or build with
/// `DISCORD_CLIENT_ID=...`. Empty = no built-in default (per-user id only).
const DEFAULT_CLIENT_ID: &str = match option_env!("DISCORD_CLIENT_ID") {
    Some(v) => v,
    None => "1518732494692810763",
};

struct State {
    /// Per-user override from Ajustes; empty falls back to `DEFAULT_CLIENT_ID`.
    override_id: String,
    client: Option<DiscordIpcClient>,
    /// Consecutive failed connection attempts, driving `BACKOFF_SECS`.
    failures: u32,
    /// Earliest time a new connection attempt may be made.
    next_attempt: Option<std::time::Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    override_id: String::new(),
    client: None,
    failures: 0,
    next_attempt: None,
});

/// Lock the state without ever panicking: this is reached from the playtime
/// watcher thread, and `panic = "abort"` would take the whole app down with it.
fn state_lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The id actually used: the user's override if set, else the embedded default.
fn effective(s: &State) -> &str {
    if s.override_id.is_empty() {
        DEFAULT_CLIENT_ID
    } else {
        &s.override_id
    }
}

/// Set/replace the per-user override client id (from Ajustes or startup). Drops
/// any open connection so the next presence update reconnects with the right id.
pub fn set_client_id(id: &str) {
    let mut s = state_lock();
    if s.override_id == id.trim() {
        return;
    }
    if let Some(mut c) = s.client.take() {
        let _ = c.close();
    }
    s.override_id = id.trim().to_string();
    s.failures = 0;
    s.next_attempt = None;
}

/// True if Rich Presence is configured (a built-in or per-user id is set).
fn configured(s: &State) -> bool {
    !effective(s).is_empty()
}

/// Backoff schedule for reconnects, in seconds.
///
/// The watcher retries presence on every poll for as long as a game runs, so
/// with Discord closed this used to open a named pipe every 5 seconds forever,
/// silently. Now failures back off to one attempt every 5 minutes and are
/// logged once.
const BACKOFF_SECS: &[u64] = &[5, 10, 30, 60, 300];

/// Ensure there's a live connection; returns false if it can't connect.
fn ensure(s: &mut State) -> bool {
    if !configured(s) {
        return false;
    }
    if s.client.is_some() {
        return true;
    }
    // Still inside the backoff window from the last failure → don't even try.
    let now = std::time::Instant::now();
    if let Some(next) = s.next_attempt {
        if now < next {
            return false;
        }
    }
    let connected = match DiscordIpcClient::new(effective(s)) {
        Ok(mut c) => {
            if c.connect().is_ok() {
                s.client = Some(c);
                true
            } else {
                false
            }
        }
        Err(_) => false,
    };
    if connected {
        s.failures = 0;
        s.next_attempt = None;
    } else {
        let idx = (s.failures as usize).min(BACKOFF_SECS.len() - 1);
        let wait = BACKOFF_SECS[idx];
        if s.failures == 0 {
            eprintln!(
                "[discord] Rich Presence could not connect (is Discord running?); retrying with backoff"
            );
        }
        s.failures = s.failures.saturating_add(1);
        s.next_attempt = Some(now + std::time::Duration::from_secs(wait));
    }
    connected
}

/// Show "playing `name`" with an elapsed timer from `started_at` (unix secs).
/// Returns true if it was sent (so the caller can avoid retrying).
pub fn set_playing(name: &str, started_at: u64) -> bool {
    if !enabled() {
        return false;
    }
    let mut s = state_lock();
    if !ensure(&mut s) {
        return false;
    }
    let timestamps = activity::Timestamps::new().start(started_at as i64);
    let act = activity::Activity::new()
        .details(name)
        .state("Jugando")
        .timestamps(timestamps);
    let act = if LARGE_IMAGE.is_empty() {
        act
    } else {
        act.assets(
            activity::Assets::new()
                .large_image(LARGE_IMAGE)
                .large_text("Astrail"),
        )
    };
    // `ensure` returning true means the client is connected, but this runs on the
    // playtime watcher thread and `panic = "abort"` turns any unwrap into a process
    // kill, so the impossible case degrades instead of asserting.
    let Some(client) = s.client.as_mut() else {
        return false;
    };
    if client.set_activity(act).is_ok() {
        true
    } else {
        // Connection dropped — discard it so the next call reconnects.
        let _ = s.client.take().map(|mut c| c.close());
        false
    }
}

/// Clear the presence (no game running).
pub fn clear() {
    let mut s = state_lock();
    if let Some(client) = s.client.as_mut() {
        let _ = client.clear_activity();
    }
}
