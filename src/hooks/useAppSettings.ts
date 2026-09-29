// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useSyncExternalStore } from 'react';

import { onEvent } from '@/lib/events';
import { createSettingsStore, EMPTY_SNAPSHOT } from '@/lib/settingsStore';
import { getAppSettings, patchAppSettings } from '@/lib/tauri';

/** This window's settings (see `settingsStore`). Starts reading on first use. */
const store = createSettingsStore({
  load: getAppSettings,
  save: patchAppSettings,
  watch: (changed) => {
    // For the life of the window, like the store itself.
    onEvent('settings-updated', changed).catch(() => {});
  },
});

/** Change only the given settings: shown at once, then saved. Rejects (after
 *  putting back what the core has) when the core refuses. */
export const patchSettings = store.patch;

/**
 * The app settings, the same copy for every component of the window, kept in
 * step with changes made anywhere (another window, the overlay hotkey, a
 * restored backup). `settings` is null until the first read answers; `error`
 * says why when that first read failed.
 */
export function useAppSettings() {
  const { settings, error } = useSyncExternalStore(store.subscribe, store.getSnapshot, () => EMPTY_SNAPSHOT);
  return { settings, error };
}
