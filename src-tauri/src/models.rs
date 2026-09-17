use serde::{Deserialize, Serialize};

/// Where a library entry comes from. Each variant is produced by its own scanner
/// module (`steam.rs`, `epic.rs`, `gog.rs`, …) and merged in `get_library`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GameSource {
    Steam,
    Epic,
    Gog,
    Ea,
    Ubisoft,
    Xbox,
    Battlenet,
    Riot,
    Rockstar,
    Amazon,
    Battlestate,
    /// Generic catch-all: anything found via the Windows uninstall registry that
    /// isn't claimed by a more specific scanner.
    Windows,
    /// A non-game application auto-detected from the uninstall registry (browsers,
    /// office suites, dev tools, media players…). Classified in `windows_apps.rs`.
    App,
    Manual,
}

/// A user-created category, optionally with a chosen icon key (resolved to an
/// SVG on the frontend from a bundled icon set). Persisted in `storage.rs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
}

/// A single launchable entry in the unified library.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    /// Stable unique id, e.g. "steam:440", "epic:Fortnite", "manual:1718900000000".
    pub id: String,
    pub name: String,
    pub source: GameSource,

    /// Steam AppID (only for `GameSource::Steam`).
    #[serde(default)]
    pub app_id: Option<u32>,

    /// Absolute path to the executable, when the game is launched directly
    /// (manual apps, GOG, Xbox, some EA games).
    #[serde(default)]
    pub executable: Option<String>,

    /// Install directory on disk, when known.
    #[serde(default)]
    pub install_dir: Option<String>,

    /// Remote cover image URL. For Steam this is the vertical capsule; for other
    /// stores it is resolved lazily via `art.rs` (SteamGridDB / Steam CDN).
    #[serde(default)]
    pub cover_url: Option<String>,

    /// Protocol URI used to launch store-managed games (Epic / Ubisoft), so the
    /// store handles DRM/updates/overlay. Takes precedence over `executable`.
    #[serde(default)]
    pub launch_uri: Option<String>,

    /// User overlay: whether the user marked this entry as a favorite. Not set by
    /// scanners; applied in `get_library` from `favorites.json` (`storage.rs`).
    #[serde(default)]
    pub favorite: bool,

    /// User overlay: manual categories the user assigned to this entry. Not set by
    /// scanners; applied in `get_library` from `categories.json` (`storage.rs`).
    #[serde(default)]
    pub categories: Vec<String>,
}

/// Global settings configured by the user or the app itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// Whether the user has completed the first-run onboarding setup.
    #[serde(default)]
    pub setup_completed: bool,
    /// Whether the app hides to the system tray when the main window is closed.
    #[serde(default = "default_minimize_to_tray")]
    pub minimize_to_tray: bool,
    /// In-game metrics overlay configuration.
    #[serde(default)]
    pub overlay: OverlaySettings,
    /// Global keybindings for the application.
    #[serde(default)]
    pub shortcuts: ShortcutsSettings,
    /// UI language: "system" (follow the OS, fallback English), "es" or "en".
    #[serde(default = "default_language")]
    pub language: String,
    /// Whether to publish the game being played to Discord Rich Presence.
    ///
    /// Defaults to **off**: this broadcasts what the user is playing to their whole
    /// friends list, so it is opt-in. `#[serde(default)]` means an existing
    /// settings file without the field also reads as off.
    #[serde(default)]
    pub discord_enabled: bool,
}

fn default_language() -> String {
    "system".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutsSettings {
    #[serde(default = "default_shortcut_spotlight")]
    pub spotlight: String,
    #[serde(default = "default_shortcut_overlay_toggle")]
    pub overlay_toggle: String,
    #[serde(default = "default_shortcut_overlay_settings")]
    pub overlay_settings: String,
}

impl Default for ShortcutsSettings {
    fn default() -> Self {
        Self {
            spotlight: default_shortcut_spotlight(),
            overlay_toggle: default_shortcut_overlay_toggle(),
            overlay_settings: default_shortcut_overlay_settings(),
        }
    }
}

impl ShortcutsSettings {
    /// Move anyone still on the original bare-function-key defaults onto the
    /// modified ones.
    ///
    /// `RegisterHotKey` takes a combination away from every application on the
    /// machine, and the old defaults were `F9`/`F10`/`F11`: quicksave in a large
    /// slice of PC games, the menu key in every Win32 app, and fullscreen in every
    /// browser. Nobody chose those, so replacing an exact match is safe; anything
    /// the user actually customised is left untouched.
    pub fn migrate_legacy_defaults(&mut self) {
        for (value, legacy, replacement) in [
            (&mut self.spotlight, "F9", default_shortcut_spotlight()),
            (&mut self.overlay_toggle, "F10", default_shortcut_overlay_toggle()),
            (
                &mut self.overlay_settings,
                "F11",
                default_shortcut_overlay_settings(),
            ),
        ] {
            if value.eq_ignore_ascii_case(legacy) {
                *value = replacement;
            }
        }
    }
}

// Modified combinations on purpose: see `migrate_legacy_defaults`. Ctrl+Shift is
// used rather than Alt+Shift (which Windows itself uses to switch keyboard layout)
// or Ctrl+Alt (which is AltGr on Spanish and other layouts).
fn default_shortcut_spotlight() -> String {
    "Ctrl+Shift+F9".to_string()
}
fn default_shortcut_overlay_toggle() -> String {
    "Ctrl+Shift+F10".to_string()
}
fn default_shortcut_overlay_settings() -> String {
    "Ctrl+Shift+F11".to_string()
}

fn default_minimize_to_tray() -> bool {
    true
}

/// Configuration for the in-game performance/telemetry overlay. Persisted as part
/// of `AppSettings`; applied live by `metrics.rs` and read by the overlay window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlaySettings {
    /// Master switch. When off, the sampler idles and the overlay window stays hidden.
    #[serde(default)]
    pub enabled: bool,
    /// Corner of the screen: "top-left" | "top-right" | "bottom-left" | "bottom-right".
    #[serde(default = "default_overlay_position")]
    pub position: String,
    /// Sampling/emit interval in milliseconds.
    #[serde(default = "default_overlay_interval")]
    pub interval_ms: u64,
    #[serde(default = "yes")]
    pub show_fps: bool,
    #[serde(default = "yes")]
    pub show_frametime: bool,
    #[serde(default = "yes")]
    pub show_gpu: bool,
    #[serde(default = "yes")]
    pub show_gpu_temp: bool,
    /// CPU temperature (needs the LHM sidecar + admin). Off by default since it
    /// requires elevation; users opt in.
    #[serde(default)]
    pub show_cpu_temp: bool,
    #[serde(default = "yes")]
    pub show_vram: bool,
    #[serde(default = "yes")]
    pub show_cpu: bool,
    #[serde(default = "yes")]
    pub show_ram: bool,
    /// Which GPU to sample: "auto" | "nvml:<i>" | "pci:<AMD PnP id fragment>".
    #[serde(default = "default_overlay_gpu")]
    pub gpu: String,
    /// Color of metric labels ("FPS", "GPU"…). CSS hex color.
    #[serde(default = "default_label_color")]
    pub label_color: String,
    /// Color of metric values (numbers). CSS hex color.
    #[serde(default = "default_value_color")]
    pub value_color: String,
    /// Accent color for FPS, GPU%, CPU% and the game title. CSS hex color.
    #[serde(default = "default_accent_color")]
    pub accent_color: String,
    /// Background opacity of the HUD panel, 0–100.
    #[serde(default = "default_bg_opacity")]
    pub bg_opacity: u8,
    /// Font-size key: "xs" | "sm" | "base".
    #[serde(default = "default_font_size_key")]
    pub font_size: String,
    /// What to do when the HUD can't get a hardware overlay plane and DWM has to
    /// composite it (which drops the game out of independent-flip → FPS/latency cost):
    /// "always" (draw regardless — current behavior) | "performance" (auto-hide the HUD
    /// once a stable *composed* state is detected, so it never silently costs FPS).
    #[serde(default = "default_mpo_mode")]
    pub mpo_mode: String,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            position: default_overlay_position(),
            interval_ms: default_overlay_interval(),
            show_fps: true,
            show_frametime: true,
            show_gpu: true,
            show_gpu_temp: true,
            show_cpu_temp: false,
            show_vram: true,
            show_cpu: true,
            show_ram: true,
            gpu: default_overlay_gpu(),
            label_color: default_label_color(),
            value_color: default_value_color(),
            accent_color: default_accent_color(),
            bg_opacity: default_bg_opacity(),
            font_size: default_font_size_key(),
            mpo_mode: default_mpo_mode(),
        }
    }
}

impl OverlaySettings {
    /// Reset a GPU choice saved by the ADLX build (`adlx:<i>`). Those indices meant
    /// nothing outside ADLX and the picker has no such option any more.
    pub fn migrate_legacy_gpu(&mut self) {
        if self.gpu.starts_with("adlx:") {
            self.gpu = default_overlay_gpu();
        }
    }
}

fn default_overlay_position() -> String {
    "top-left".to_string()
}

fn default_mpo_mode() -> String {
    "always".to_string()
}

fn default_overlay_gpu() -> String {
    "auto".to_string()
}

fn default_overlay_interval() -> u64 {
    1000
}

fn default_label_color() -> String {
    "#8a8a8a".to_string()
}

fn default_value_color() -> String {
    "#ffffff".to_string()
}

fn default_accent_color() -> String {
    "#ef4444".to_string()
}

fn default_bg_opacity() -> u8 {
    85
}

fn default_font_size_key() -> String {
    "sm".to_string()
}

fn yes() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bare_function_key_defaults_are_migrated_to_modified_ones() {
        let mut s = ShortcutsSettings {
            spotlight: "F9".into(),
            overlay_toggle: "f10".into(),
            overlay_settings: "F11".into(),
        };
        s.migrate_legacy_defaults();
        assert_eq!(s.spotlight, "Ctrl+Shift+F9");
        // Case-insensitive: the value is whatever a previous build wrote.
        assert_eq!(s.overlay_toggle, "Ctrl+Shift+F10");
        assert_eq!(s.overlay_settings, "Ctrl+Shift+F11");
    }

    #[test]
    fn a_shortcut_the_user_chose_is_never_rewritten() {
        let mut s = ShortcutsSettings {
            spotlight: "Alt+Space".into(),
            overlay_toggle: "F12".into(),
            overlay_settings: String::new(),
        };
        s.migrate_legacy_defaults();
        assert_eq!(s.spotlight, "Alt+Space");
        assert_eq!(s.overlay_toggle, "F12");
        assert_eq!(s.overlay_settings, "");
    }

    #[test]
    fn no_default_shortcut_is_a_bare_key() {
        // A bare key is taken from every application on the machine, games included.
        for combo in [
            default_shortcut_spotlight(),
            default_shortcut_overlay_toggle(),
            default_shortcut_overlay_settings(),
        ] {
            assert!(combo.contains('+'), "{combo} has no modifier");
        }
    }

    #[test]
    fn an_adlx_gpu_choice_is_reset_to_auto() {
        let mut o = OverlaySettings { gpu: "adlx:1".into(), ..Default::default() };
        o.migrate_legacy_gpu();
        assert_eq!(o.gpu, "auto");
        for kept in ["nvml:1", "pci:VEN_1002&DEV_7550&SUBSYS_88111EAE&REV_C0"] {
            let mut o = OverlaySettings { gpu: kept.into(), ..Default::default() };
            o.migrate_legacy_gpu();
            assert_eq!(o.gpu, kept);
        }
    }

    #[test]
    fn rich_presence_is_off_until_the_user_asks_for_it() {
        // Deserializing a settings file written before the field existed must not
        // silently start broadcasting what the user is playing.
        let s: AppSettings = serde_json::from_str("{}").expect("defaults");
        assert!(!s.discord_enabled);
    }
}

