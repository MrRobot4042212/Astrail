// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! System tray icon and its menu.
//!
//! Astrail lives in the tray so the watchers keep running after the window is
//! closed: left click or "Show Astrail" reopens the window, "Quit" really quits.
//!
//! The menu is native, so it cannot use the webview's catalogs. It carries its
//! own two strings per language and follows `AppSettings.language` the same way
//! the webview does (`src/i18n/config.ts::resolveLanguage`): "es" / "en" as
//! chosen, "system" = Spanish when the Windows UI language is Spanish, English
//! otherwise. Everything here runs on the main thread (setup and
//! `settings_changed`, which only calls `set_language` through
//! `run_on_main_thread`).

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

/// Menu items kept so their text can change with the language.
struct TrayItems {
    show: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lang {
    Es,
    En,
}

/// `(show, quit)` labels for a language.
fn labels(lang: Lang) -> (&'static str, &'static str) {
    match lang {
        Lang::Es => ("Mostrar Astrail", "Salir"),
        Lang::En => ("Show Astrail", "Quit"),
    }
}

/// Mirror of the webview's `resolveLanguage`, with the OS language injected so
/// it can be tested.
fn resolve(setting: &str, system_is_spanish: bool) -> Lang {
    match setting {
        "es" => Lang::Es,
        "en" => Lang::En,
        _ if system_is_spanish => Lang::Es,
        _ => Lang::En,
    }
}

/// Whether the Windows display language is Spanish (any region).
pub(crate) fn system_is_spanish() -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::Globalization::GetUserDefaultUILanguage;
        const LANG_SPANISH: u16 = 0x0a;
        // SAFETY: no arguments, no pointers; returns the current user's UI LANGID.
        let langid = unsafe { GetUserDefaultUILanguage() };
        langid & 0x3ff == LANG_SPANISH
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Build the tray icon. Called once from `setup`.
pub fn build(app: &AppHandle, language: &str) -> tauri::Result<()> {
    let (show_label, quit_label) = labels(resolve(language, system_is_spanish()));
    let show = MenuItem::with_id(app, "show", show_label, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", quit_label, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    // No `unwrap()`: a missing icon must not take the whole app down at
    // startup — the tray just shows the default one.
    let mut tray = TrayIconBuilder::with_id("main");
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.tooltip("Astrail")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => crate::show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::show_main(tray.app_handle());
            }
        })
        .build(app)?;
    app.manage(TrayItems { show, quit });
    Ok(())
}

/// Relabel the menu after the language setting changed. Main thread only.
pub fn set_language(app: &AppHandle, language: &str) {
    let Some(items) = app.try_state::<TrayItems>() else {
        return;
    };
    let (show_label, quit_label) = labels(resolve(language, system_is_spanish()));
    if let Err(err) = items.show.set_text(show_label).and_then(|_| items.quit.set_text(quit_label)) {
        log::warn!("tray menu relabel failed: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tray_follows_the_same_language_rule_as_the_webview() {
        assert_eq!(resolve("es", false), Lang::Es);
        assert_eq!(resolve("en", true), Lang::En);
        assert_eq!(resolve("system", true), Lang::Es);
        assert_eq!(resolve("system", false), Lang::En);
        // An unknown stored value behaves like "system", as in `resolveLanguage`.
        assert_eq!(resolve("fr", false), Lang::En);
    }

    #[test]
    fn every_language_has_both_labels() {
        for lang in [Lang::Es, Lang::En] {
            let (show, quit) = labels(lang);
            assert!(!show.is_empty() && !quit.is_empty());
        }
    }
}
