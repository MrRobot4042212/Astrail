// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

// Everything the settings drawer knows and can do. The drawer renders the
// shell, each tab reads what it needs from the model; nothing is drilled
// through as a prop list.

import { useCallback, useEffect, useRef, useState } from 'react';
import {
  clearCoverCache,
  hiddenCount,
  restoreHidden,
  getDiscordClientId,
  setDiscordClientId,
  getAutostart,
  setAutostart,
  getAppSettings,
  patchAppSettings,
  systemInfo,
  isElevated,
  restartAsAdmin,
} from '@/lib/tauri';
import type { OverlaySettings, ShortcutsSettings, SystemInfo } from '@/lib/types';

/** How long the "saved" confirmation stays next to the Discord field. */
const SAVED_FLASH_MS = 2000;

export interface SettingsModel {
  /** A cache wipe, a restore or a Discord save is in flight. */
  busy: boolean;
  error: string | null;
  /** `null` = not loaded, or the read failed: the control stays hidden. */
  hidden: number | null;
  discordId: string;
  discordSaved: boolean;
  discordEnabled: boolean | null;
  autostart: boolean | null;
  tray: boolean | null;
  overlay: OverlaySettings | null;
  shortcuts: ShortcutsSettings | null;
  sys: SystemInfo | null;
  elevated: boolean | null;
  language: string;
  setDiscordId: (id: string) => void;
  updateOverlay: (patch: Partial<OverlaySettings>) => void;
  updateShortcuts: (patch: Partial<ShortcutsSettings>) => void;
  saveLanguage: (lang: string) => void;
  toggleAutostart: () => void;
  toggleTray: () => void;
  toggleDiscord: () => void;
  saveDiscord: () => void;
  clear: () => void;
  restore: () => void;
  restartAdmin: () => void;
}

export function useSettingsModel({
  onChanged,
  onClose,
}: {
  /** Called after a change (cache wiped / hidden restored) so the library can refresh. */
  onChanged: () => void;
  onClose: () => void;
}): SettingsModel {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hidden, setHidden] = useState<number | null>(null);
  const [discordId, setDiscordId] = useState('');
  const [discordSaved, setDiscordSaved] = useState(false);
  const [discordEnabled, setDiscordEnabled] = useState<boolean | null>(null);
  const [autostart, setAutostartState] = useState<boolean | null>(null);
  const [tray, setTray] = useState<boolean | null>(null);
  const [overlay, setOverlay] = useState<OverlaySettings | null>(null);
  const [shortcuts, setShortcuts] = useState<ShortcutsSettings | null>(null);
  const [sys, setSys] = useState<SystemInfo | null>(null);
  const [elevated, setElevated] = useState<boolean | null>(null);
  const [language, setLanguageState] = useState<string>('system');
  const savedFlash = useRef<number | null>(null);

  useEffect(() => {
    systemInfo()
      .then(setSys)
      .catch(() => setSys(null));
    isElevated()
      .then(setElevated)
      .catch(() => setElevated(null));
    hiddenCount()
      .then(setHidden)
      .catch(() => setHidden(null));
    getDiscordClientId()
      .then(setDiscordId)
      .catch(() => {});
    getAutostart()
      .then(setAutostartState)
      .catch(() => setAutostartState(null));
    getAppSettings()
      .then((s) => {
        setTray(s.minimize_to_tray);
        setOverlay(s.overlay);
        setShortcuts(s.shortcuts);
        setLanguageState(s.language ?? 'system');
        setDiscordEnabled(s.discord_enabled ?? false);
      })
      .catch(() => setTray(null));
    return () => {
      if (savedFlash.current !== null) window.clearTimeout(savedFlash.current);
    };
  }, []);

  // Optimistic local update, then send only the changed fields; Rust merges them.
  const updateOverlay = useCallback((patch: Partial<OverlaySettings>) => {
    setOverlay((prev) => (prev ? { ...prev, ...patch } : prev));
    patchAppSettings({ overlay: patch }).catch((e) => setError(String(e)));
  }, []);

  const updateShortcuts = useCallback((patch: Partial<ShortcutsSettings>) => {
    setShortcuts((prev) => (prev ? { ...prev, ...patch } : prev));
    patchAppSettings({ shortcuts: patch }).catch((e) => setError(String(e)));
  }, []);

  const saveLanguage = useCallback((lang: string) => {
    setLanguageState(lang); // optimistic
    // patch_app_settings emits "settings-updated", which the I18nProvider listens
    // to and applies the new language across every window.
    patchAppSettings({ language: lang }).catch((e) => setError(String(e)));
  }, []);

  async function toggleAutostart() {
    if (autostart === null) return;
    const next = !autostart;
    setAutostartState(next); // optimistic
    try {
      await setAutostart(next);
    } catch (e) {
      setAutostartState(!next); // revert
      setError(String(e));
    }
  }

  async function toggleTray() {
    if (tray === null) return;
    const next = !tray;
    setTray(next); // optimistic
    try {
      await patchAppSettings({ minimize_to_tray: next });
    } catch (e) {
      setTray(!next); // revert
      setError(String(e));
    }
  }

  async function toggleDiscord() {
    if (discordEnabled === null) return;
    const next = !discordEnabled;
    setDiscordEnabled(next); // optimistic
    try {
      await patchAppSettings({ discord_enabled: next });
    } catch (e) {
      setDiscordEnabled(!next); // revert
      setError(String(e));
    }
  }

  async function saveDiscord() {
    setBusy(true);
    setError(null);
    try {
      await setDiscordClientId(discordId.trim());
      setDiscordSaved(true);
      if (savedFlash.current !== null) window.clearTimeout(savedFlash.current);
      savedFlash.current = window.setTimeout(() => {
        savedFlash.current = null;
        setDiscordSaved(false);
      }, SAVED_FLASH_MS);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  /** Run a library-changing action, then hand over to the library and close. */
  async function thenRefresh(action: () => Promise<unknown>) {
    setBusy(true);
    setError(null);
    try {
      await action();
      onChanged();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function restartAdmin() {
    setError(null);
    try {
      await restartAsAdmin(); // app relaunches elevated and this instance exits
    } catch (e) {
      setError(String(e));
    }
  }

  return {
    busy,
    error,
    hidden,
    discordId,
    discordSaved,
    discordEnabled,
    autostart,
    tray,
    overlay,
    shortcuts,
    sys,
    elevated,
    language,
    setDiscordId,
    updateOverlay,
    updateShortcuts,
    saveLanguage,
    toggleAutostart,
    toggleTray,
    toggleDiscord,
    saveDiscord,
    clear: () => thenRefresh(clearCoverCache),
    restore: () => thenRefresh(restoreHidden),
    restartAdmin,
  };
}
