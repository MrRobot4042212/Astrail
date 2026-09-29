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
  clearRunAsAdmin,
  systemInfo,
  metricsAccess,
  restartAsAdmin,
} from '@/lib/tauri';
import type { AppSettingsPatch, MetricsAccess, OverlaySettings, ShortcutsSettings, SystemInfo } from '@/lib/types';
import { failureText } from '@/i18n/failureText';
import { patchSettings, useAppSettings } from '@/hooks/useAppSettings';

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
  /** False when this copy is not installed: autostart cannot point at it. */
  autostartAvailable: boolean;
  /** The executable is marked "Run as administrator": Windows skips it at sign-in. */
  autostartBlocked: boolean;
  tray: boolean | null;
  overlay: OverlaySettings | null;
  shortcuts: ShortcutsSettings | null;
  sys: SystemInfo | null;
  /** `null` = not loaded, or the read failed: the permissions panel stays hidden. */
  access: MetricsAccess | null;
  language: string;
  setDiscordId: (id: string) => void;
  updateOverlay: (patch: Partial<OverlaySettings>) => void;
  updateShortcuts: (patch: Partial<ShortcutsSettings>) => void;
  saveLanguage: (lang: string) => void;
  toggleAutostart: () => void;
  /** Remove the "Run as administrator" flag so the Run entry works again. */
  unblockAutostart: () => void;
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
  const [autostart, setAutostartState] = useState<boolean | null>(null);
  const [autostartAvailable, setAutostartAvailable] = useState(false);
  const [autostartBlocked, setAutostartBlocked] = useState(false);
  const [sys, setSys] = useState<SystemInfo | null>(null);
  const [access, setAccess] = useState<MetricsAccess | null>(null);
  // The window's shared settings, not a copy: a change made elsewhere while the
  // dialog is open (the overlay hotkey, another window) shows here too (G8).
  const { settings } = useAppSettings();
  const tray = settings?.minimize_to_tray ?? null;
  const overlay = settings?.overlay ?? null;
  const shortcuts = settings?.shortcuts ?? null;
  const language = settings?.language ?? 'system';
  const discordEnabled = settings ? (settings.discord_enabled ?? false) : null;
  const savedFlash = useRef<number | null>(null);

  useEffect(() => {
    systemInfo()
      .then(setSys)
      .catch(() => setSys(null));
    metricsAccess()
      .then(setAccess)
      .catch(() => setAccess(null));
    hiddenCount()
      .then(setHidden)
      .catch(() => setHidden(null));
    getDiscordClientId()
      .then(setDiscordId)
      .catch(() => {});
    getAutostart()
      .then((s) => {
        setAutostartState(s.enabled);
        setAutostartAvailable(s.available);
        setAutostartBlocked(s.blocked_by_run_as_admin);
      })
      .catch(() => setAutostartState(null));
    return () => {
      if (savedFlash.current !== null) window.clearTimeout(savedFlash.current);
    };
  }, []);

  // Only the changed fields; Rust merges them. The store shows the change at
  // once and puts back the saved value if the core refuses.
  const save = useCallback((patch: AppSettingsPatch) => {
    setError(null);
    patchSettings(patch).catch((e) => setError(failureText(e)));
  }, []);

  const updateOverlay = useCallback((patch: Partial<OverlaySettings>) => save({ overlay: patch }), [save]);

  const updateShortcuts = useCallback((patch: Partial<ShortcutsSettings>) => save({ shortcuts: patch }), [save]);

  // The I18nProvider follows the store, so the language switches at once here
  // and, through `settings-updated`, in every other window.
  const saveLanguage = useCallback((lang: string) => save({ language: lang }), [save]);

  async function toggleAutostart() {
    if (autostart === null || !autostartAvailable) return;
    const next = !autostart;
    setAutostartState(next); // optimistic
    try {
      await setAutostart(next);
    } catch (e) {
      setAutostartState(!next); // revert
      setError(failureText(e));
    }
  }

  async function unblockAutostart() {
    setError(null);
    try {
      const s = await clearRunAsAdmin();
      setAutostartState(s.enabled);
      setAutostartAvailable(s.available);
      setAutostartBlocked(s.blocked_by_run_as_admin);
    } catch (e) {
      setError(failureText(e));
    }
  }

  function toggleTray() {
    if (tray !== null) save({ minimize_to_tray: !tray });
  }

  function toggleDiscord() {
    if (discordEnabled !== null) save({ discord_enabled: !discordEnabled });
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
      setError(failureText(e));
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
      setError(failureText(e));
    } finally {
      setBusy(false);
    }
  }

  async function restartAdmin() {
    setError(null);
    try {
      await restartAsAdmin(); // app relaunches elevated and this instance exits
    } catch (e) {
      setError(failureText(e));
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
    autostartAvailable,
    autostartBlocked,
    tray,
    overlay,
    shortcuts,
    sys,
    access,
    language,
    setDiscordId,
    updateOverlay,
    updateShortcuts,
    saveLanguage,
    toggleAutostart,
    unblockAutostart,
    toggleTray,
    toggleDiscord,
    saveDiscord,
    clear: () => thenRefresh(clearCoverCache),
    restore: () => thenRefresh(restoreHidden),
    restartAdmin,
  };
}
