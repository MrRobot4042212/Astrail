// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

//! Filesystem helpers for the detail page (folder size, "open folder").
//!
//! Everything here takes a path that Rust resolved itself — the commands in
//! `lib.rs` take a **game id** and read `install_dir` from the library cache, so
//! the webview can never hand us an arbitrary path. The validation below is
//! defence in depth on top of that, plus the traversal caps that keep
//! `dir_size` from walking a whole drive.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Directory-tree depth cap for `dir_size`. Deep enough for any real game
/// install, shallow enough that a junction loop or a mount point pointing at the
/// drive root cannot turn one IPC call into an unbounded walk.
const MAX_DEPTH: u32 = 24;
/// Hard cap on visited entries, for the same reason.
const MAX_ENTRIES: u64 = 500_000;

/// Absolute path to a Windows system binary. System binaries are resolved from
/// `%SystemRoot%\System32` and never from `PATH`: Astrail can be running
/// elevated, and a binary planted in a writable `PATH` entry would inherit that
/// token.
pub fn system_exe(name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        // explorer.exe lives in %SystemRoot%, the rest in %SystemRoot%\System32.
        let root = PathBuf::from(root);
        let direct = root.join(name);
        if direct.is_file() {
            return direct;
        }
        root.join("System32").join(name)
    }
    #[cfg(not(windows))]
    {
        PathBuf::from(name)
    }
}

/// Strip the `\\?\` verbatim prefix that `canonicalize` adds on Windows;
/// Explorer and several game launchers reject verbatim paths.
pub fn strip_verbatim(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path.to_path_buf(),
    }
}

/// Resolve a path and require it to be an existing directory.
pub fn validate_dir(path: &str) -> Result<PathBuf, String> {
    let canon =
        fs::canonicalize(path).map_err(|e| format!("No se pudo resolver «{path}»: {e}"))?;
    if !canon.is_dir() {
        return Err(format!("«{path}» no es una carpeta"));
    }
    Ok(canon)
}

/// Total size in bytes of a directory tree. Walks iteratively, skips entries it
/// can't read instead of failing the whole call, and stops at `MAX_DEPTH` /
/// `MAX_ENTRIES` so a junction loop can't spin forever.
pub fn dir_size(path: &Path) -> u64 {
    let mut total: u64 = 0;
    let mut visited: u64 = 0;
    let mut stack: Vec<(PathBuf, u32)> = vec![(path.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > MAX_ENTRIES {
                return total;
            }
            // `file_type` does not follow symlinks/junctions, so a reparse point
            // is neither descended into nor counted twice.
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                if depth < MAX_DEPTH {
                    stack.push((entry.path(), depth + 1));
                }
            } else if ft.is_file() {
                if let Ok(md) = entry.metadata() {
                    total += md.len();
                }
            }
        }
    }
    total
}

/// How a shell-out from Astrail reaches the desktop.
#[derive(Debug, PartialEq, Eq)]
pub enum OpenRoute {
    /// Astrail runs with the user's own token: spawn/`ShellExecuteW` directly.
    Direct,
    /// Astrail is elevated: hand the request to the desktop Explorer so the
    /// child (Explorer window, browser) gets the user's medium token instead of
    /// inheriting administrator (see `launcher::desktop_shell`).
    DesktopShell,
}

/// Route for anything opened from the detail page. Same rule as game launches:
/// an elevated Astrail never spawns a shell-out itself.
pub fn open_route(elevated: bool) -> OpenRoute {
    if elevated {
        OpenRoute::DesktopShell
    } else {
        OpenRoute::Direct
    }
}

#[cfg(target_os = "windows")]
fn elevated() -> bool {
    crate::elevation::is_elevated()
}

/// Open a validated directory in the OS file manager.
pub fn open_folder(dir: &Path) -> Result<(), String> {
    let path = strip_verbatim(dir);
    #[cfg(target_os = "windows")]
    {
        match open_route(elevated()) {
            OpenRoute::DesktopShell => {
                // Explorer opens a folder path with its default verb, so no
                // `explorer.exe` argument is needed — and the window belongs to
                // the unelevated shell process.
                let target = path.to_string_lossy();
                crate::launcher::desktop_shell::open(&target, None, None)?;
            }
            OpenRoute::Direct => {
                Command::new(system_exe("explorer.exe"))
                    .arg(&path)
                    .spawn()
                    .map_err(|e| format!("No se pudo abrir «{}»: {e}", path.display()))?;
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("No se pudo abrir «{}»: {e}", path.display()))?;
    }
    Ok(())
}

/// Hosts the detail page is allowed to open in the user's browser. The URLs are
/// built from fixed templates with only the game name interpolated, so an
/// allowlist is an exact fit — and it keeps a poisoned library entry from
/// turning "open community links" into "open anything".
///
/// `github.com` is here for the project links in Settings → About, which the
/// additional terms of the license require the app to keep showing.
const EXTERNAL_HOSTS: &[&str] = &[
    "github.com",
    "pcgamingwiki.com",
    "nexusmods.com",
    "protondb.com",
    "duckduckgo.com",
    "youtube.com",
    "twitch.tv",
    "reddit.com",
    "speedrun.com",
    "howlongtobeat.com",
];

/// Host of an `https://` URL, lowercased, or `None` if it is not a plain https
/// URL we can parse without pulling in a URL crate.
pub(crate) fn https_host(url: &str) -> Option<String> {
    if url.chars().any(|c| c.is_control() || c == '"' || c == ' ') {
        return None;
    }
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    // No credentials in the authority: `https://evil.com@allowed.com/` reads as
    // "allowed" to a human and as "evil.com" to nothing at all here.
    if authority.contains('@') || authority.is_empty() {
        return None;
    }
    let host = authority.split(':').next()?.to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Whether this URL may be opened in the user's browser.
pub fn is_allowed_external(url: &str) -> bool {
    let Some(host) = https_host(url) else {
        return false;
    };
    EXTERNAL_HOSTS
        .iter()
        .any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")))
}

/// Open an allowlisted external URL in the default browser.
///
/// The detail page used bare `<a href>` links, which navigate the **webview
/// itself** away from the app (there is no browser chrome to come back with).
pub fn open_external(url: &str) -> Result<(), String> {
    if !is_allowed_external(url) {
        return Err(format!("URL externa no permitida: «{url}»"));
    }
    #[cfg(target_os = "windows")]
    {
        use windows::core::{w, HSTRING, PCWSTR};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        // An elevated Astrail must not start the browser itself: the browser
        // would inherit the administrator token for the rest of its session.
        if open_route(elevated()) == OpenRoute::DesktopShell {
            return crate::launcher::desktop_shell::open(url, None, None);
        }

        let target = HSTRING::from(url);
        // SAFETY: the string outlives the call; the URL was allowlisted above.
        let result = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        if (result.0 as isize) <= 32 {
            return Err(format!("No se pudo abrir «{url}»"));
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("No se pudo abrir «{url}»: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_urls_are_host_allowlisted() {
        assert!(is_allowed_external("https://www.pcgamingwiki.com/w/index.php?search=Halo"));
        assert!(is_allowed_external("https://duckduckgo.com/?q=x"));
        assert!(is_allowed_external("https://github.com/MrRobot4042212/Astrail"));
        // Not on the list, wrong scheme, credential trick, or a lookalike host.
        assert!(!is_allowed_external("https://evil.example/"));
        assert!(!is_allowed_external("http://www.reddit.com/"));
        assert!(!is_allowed_external("https://evil.com@youtube.com/"));
        assert!(!is_allowed_external("https://notyoutube.com/"));
        assert!(!is_allowed_external("javascript:alert(1)"));
        assert!(!is_allowed_external("https://youtube.com/\u{0}"));
    }

    #[test]
    fn an_elevated_astrail_never_opens_folders_or_links_itself() {
        // Regression (A2/W1 residual): `open_game_folder` and the community
        // links spawned Explorer / the browser directly, so from an elevated
        // Astrail they inherited the administrator token.
        assert_eq!(open_route(true), OpenRoute::DesktopShell);
        assert_eq!(open_route(false), OpenRoute::Direct);
    }

    #[test]
    fn verbatim_prefix_is_stripped() {
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\C:\Games\Foo")),
            PathBuf::from(r"C:\Games\Foo")
        );
        assert_eq!(
            strip_verbatim(Path::new(r"C:\Games\Foo")),
            PathBuf::from(r"C:\Games\Foo")
        );
    }

    #[test]
    fn validate_dir_rejects_files_and_missing_paths() {
        let dir = std::env::temp_dir();
        assert!(validate_dir(&dir.to_string_lossy()).is_ok());
        assert!(validate_dir("Z:\\definitely\\not\\here\\astrail-test").is_err());

        let file = dir.join("astrail-validate-dir.tmp");
        fs::write(&file, b"x").unwrap();
        assert!(validate_dir(&file.to_string_lossy()).is_err());
        let _ = fs::remove_file(&file);
    }

    #[test]
    fn dir_size_sums_files() {
        let dir = std::env::temp_dir().join("astrail-dir-size-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("a.bin"), vec![0u8; 1000]).unwrap();
        fs::write(dir.join("sub/b.bin"), vec![0u8; 24]).unwrap();
        assert_eq!(dir_size(&dir), 1024);
        let _ = fs::remove_dir_all(&dir);
    }
}
