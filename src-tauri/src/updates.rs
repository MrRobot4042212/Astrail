// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Update channels.
//!
//! A release is published as a GitHub *pre-release* first, so
//! `releases/latest/download/latest.json` (the stable endpoint) keeps answering
//! with the previous version. The same workflow copies the new `latest.json` to the
//! fixed `channel-beta` release, which is what opted-in installations read. A
//! separate, manual `promote` workflow later clears the pre-release flag: the very
//! binary the beta users ran becomes what `releases/latest` serves. Nothing is
//! rebuilt in between.
//!
//! The check lives here, not in the webview, because the plugin's JavaScript
//! `check()` cannot choose an endpoint (on purpose: a page must not be able to point
//! the updater somewhere else). The `Update` is stored in the webview's resource
//! table exactly as the plugin's own `check` command does, so the plugin's
//! `download` / `install` commands keep working on it unchanged.
//!
//! The updater takes the *first* endpoint that answers, it does not compare them. A
//! beta installation therefore asks both and keeps the newer offer, which also
//! covers a missing or stale beta pointer.

use std::time::Duration;

use serde::Serialize;
use tauri::{Manager, ResourceId, Url, Webview};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::models::{AppSettings, UpdateChannel};

/// Must stay equal to `plugins.updater.endpoints[0]` in `tauri.conf.json` (tested).
pub const STABLE_ENDPOINT: &str =
    "https://github.com/MrRobot4042212/Astrail/releases/latest/download/latest.json";
/// Asset of the fixed `channel-beta` release, overwritten by `release.yml`.
pub const BETA_ENDPOINT: &str =
    "https://github.com/MrRobot4042212/Astrail/releases/download/channel-beta/latest.json";

const CHECK_TIMEOUT: Duration = Duration::from_secs(15);

/// Same shape as the updater plugin's own metadata (the JavaScript `Update` class
/// is built from it), plus the channel the offer came from.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMetadata {
    rid: ResourceId,
    current_version: String,
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
    raw_json: serde_json::Value,
    channel: UpdateChannel,
}

/// Endpoints asked for a channel, in the order they are asked.
pub fn endpoints_for(channel: UpdateChannel) -> &'static [(UpdateChannel, &'static str)] {
    match channel {
        UpdateChannel::Stable => &[(UpdateChannel::Stable, STABLE_ENDPOINT)],
        UpdateChannel::Beta => &[
            (UpdateChannel::Beta, BETA_ENDPOINT),
            (UpdateChannel::Stable, STABLE_ENDPOINT),
        ],
    }
}

/// `X.Y.Z` as numbers. Releases are plain semver (the release workflow refuses
/// anything else), so a version that does not parse is never preferred.
fn parse_version(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.trim().trim_start_matches('v').split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some((major, minor, patch))
}

/// Whether `candidate` should replace `best`. Ties keep `best`, which is the
/// earlier endpoint in `endpoints_for`.
fn is_newer(candidate: &str, best: &str) -> bool {
    match (parse_version(candidate), parse_version(best)) {
        (Some(candidate), Some(best)) => candidate > best,
        (Some(_), None) => true,
        _ => false,
    }
}

async fn check_endpoint(webview: &Webview, endpoint: &str) -> Result<Option<Update>, String> {
    let url = Url::parse(endpoint).map_err(|e| format!("Invalid update endpoint: {e}"))?;
    let updater = webview
        .updater_builder()
        .endpoints(vec![url])
        .map_err(|e| e.to_string())?
        .timeout(CHECK_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    updater.check().await.map_err(|e| e.to_string())
}

/// Ask the endpoints of the configured channel and offer the newest version found.
#[tauri::command]
pub async fn check_update(
    webview: Webview,
    state: tauri::State<'_, std::sync::Mutex<AppSettings>>,
) -> Result<Option<UpdateMetadata>, String> {
    // Copied out: the guard must not live across an await.
    let channel = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .update_channel;

    let mut best: Option<(UpdateChannel, Update)> = None;
    let mut last_error = None;
    let mut answered = false;
    for (source, endpoint) in endpoints_for(channel) {
        match check_endpoint(&webview, endpoint).await {
            Ok(found) => {
                answered = true;
                if let Some(update) = found {
                    let replace = best
                        .as_ref()
                        .is_none_or(|(_, current)| is_newer(&update.version, &current.version));
                    if replace {
                        best = Some((*source, update));
                    }
                }
            }
            Err(e) => {
                // Expected for the beta pointer until the first beta is published.
                log::info!("update check on the {source:?} endpoint failed: {e}");
                last_error = Some(e);
            }
        }
    }

    let Some((source, update)) = best else {
        return match (answered, last_error) {
            (false, Some(e)) => Err(e),
            _ => Ok(None),
        };
    };
    log::info!(
        "update available: {} -> {} ({source:?} endpoint, {channel:?} channel)",
        update.current_version,
        update.version
    );
    Ok(Some(UpdateMetadata {
        current_version: update.current_version.clone(),
        version: update.version.clone(),
        // `latest.json` already carries an RFC 3339 string; the parsed value would
        // need the `time` crate only to be formatted back.
        date: update.raw_json.get("pub_date").and_then(|d| d.as_str()).map(str::to_owned),
        body: update.body.clone(),
        raw_json: update.raw_json.clone(),
        channel: source,
        rid: webview.resources_table().add(update),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stable_channel_never_asks_the_beta_pointer() {
        let asked: Vec<&str> = endpoints_for(UpdateChannel::Stable).iter().map(|(_, url)| *url).collect();
        assert_eq!(asked, [STABLE_ENDPOINT]);
    }

    #[test]
    fn the_beta_channel_also_asks_stable() {
        // The updater takes the first endpoint that answers. Without the stable
        // one, a beta installation whose pointer is missing or stale stops updating.
        let asked: Vec<&str> = endpoints_for(UpdateChannel::Beta).iter().map(|(_, url)| *url).collect();
        assert_eq!(asked, [BETA_ENDPOINT, STABLE_ENDPOINT]);
    }

    #[test]
    fn every_endpoint_is_https_on_the_project_repository() {
        for (_, endpoint) in endpoints_for(UpdateChannel::Beta) {
            let url = Url::parse(endpoint).expect("endpoint parses");
            assert_eq!(url.scheme(), "https");
            assert_eq!(url.host_str(), Some("github.com"));
            assert!(url.path().starts_with("/MrRobot4042212/Astrail/releases/"));
        }
    }

    #[test]
    fn the_stable_endpoint_matches_the_bundled_updater_config() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json parses");
        assert_eq!(
            conf["plugins"]["updater"]["endpoints"],
            serde_json::json!([STABLE_ENDPOINT])
        );
    }

    #[test]
    fn versions_compare_as_numbers_not_as_text() {
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("v1.0.0", "0.99.99"));
        assert!(!is_newer("0.3.1", "0.3.1"), "a tie keeps the earlier endpoint");
        assert!(!is_newer("0.3.0", "0.3.1"));
    }

    #[test]
    fn a_version_that_does_not_parse_is_never_preferred() {
        assert!(!is_newer("0.4.0-beta.1", "0.3.0"));
        assert!(!is_newer("garbage", "0.3.0"));
        assert!(!is_newer("1.2", "0.3.0"));
        assert!(is_newer("0.3.0", "garbage"));
    }
}
