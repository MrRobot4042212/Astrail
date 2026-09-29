// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import type { AppSettings, AppSettingsPatch } from './types';

/**
 * One copy of the app settings per window, shared by every component in it.
 *
 * Ten components used to read the settings on their own, each with its own
 * `getAppSettings()`. Three re-read on every `settings-updated` (two reads per
 * window); the other seven read once on mount and went stale. The settings
 * dialog kept showing the HUD as off after the overlay hotkey turned it on
 * (2026-09-27 audit, G8).
 *
 * The store reads once, re-reads on every change announced by the core, and
 * applies a change locally before the core confirms it. Reads and changes are
 * numbered: an answer only lands if nothing newer has landed since it was asked
 * for, so a slow read never puts back a value the user has just changed.
 */

export interface SettingsSnapshot {
  /** Null until the first read answers. */
  settings: AppSettings | null;
  /** Why the first read failed, while there is still nothing to show. */
  error: unknown;
}

/** What the store needs from the core. */
export interface SettingsBackend {
  load: () => Promise<AppSettings>;
  save: (patch: AppSettingsPatch) => Promise<void>;
  /** Call `changed` whenever the settings may have changed, from any window.
   *  Registered once, for the life of the window. */
  watch: (changed: () => void) => void;
}

export interface SettingsStore {
  subscribe: (listener: () => void) => () => void;
  getSnapshot: () => SettingsSnapshot;
  /** Read the settings again. */
  refresh: () => Promise<void>;
  /** Change only the given settings: shown at once, then saved. On failure the
   *  store re-reads what the core has and the promise rejects. */
  patch: (patch: AppSettingsPatch) => Promise<void>;
}

export const EMPTY_SNAPSHOT: SettingsSnapshot = Object.freeze({ settings: null, error: null });

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** `patch` applied to `current` the way the core applies it: nested objects
 *  merge key by key, anything else is replaced. */
export function mergeSettings(current: AppSettings, patch: AppSettingsPatch): AppSettings {
  const next: Record<string, unknown> = { ...current };
  for (const [key, value] of Object.entries(patch)) {
    if (value === undefined) continue;
    const prev = next[key];
    next[key] = isPlainObject(prev) && isPlainObject(value) ? { ...prev, ...value } : value;
  }
  return next as AppSettings;
}

export function createSettingsStore(backend: SettingsBackend): SettingsStore {
  let snapshot: SettingsSnapshot = EMPTY_SNAPSHOT;
  const listeners = new Set<() => void>();
  let started = false;
  // Numbers handed out to reads and local changes, and the newest one applied.
  let issued = 0;
  let applied = 0;

  const publish = (next: SettingsSnapshot) => {
    snapshot = next;
    for (const listener of listeners) listener();
  };

  async function refresh(): Promise<void> {
    const ticket = ++issued;
    try {
      const settings = await backend.load();
      if (ticket < applied) return;
      applied = ticket;
      publish({ settings, error: null });
    } catch (error) {
      if (ticket < applied) return;
      applied = ticket;
      // A failed re-read keeps what is on screen; only a first read reports.
      if (snapshot.settings === null) publish({ settings: null, error });
    }
  }

  async function patch(change: AppSettingsPatch): Promise<void> {
    const ticket = ++issued;
    if (snapshot.settings !== null) {
      applied = ticket;
      publish({ settings: mergeSettings(snapshot.settings, change), error: null });
    }
    try {
      await backend.save(change);
    } catch (error) {
      await refresh();
      throw error;
    }
  }

  function subscribe(listener: () => void): () => void {
    listeners.add(listener);
    if (!started) {
      started = true;
      backend.watch(() => void refresh());
      void refresh();
    }
    return () => {
      listeners.delete(listener);
    };
  }

  return { subscribe, getSnapshot: () => snapshot, refresh, patch };
}
