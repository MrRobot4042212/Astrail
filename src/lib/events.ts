// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

// Events the core sends to the webview (`src-tauri/src/events.rs`), with the
// payload each one carries. A Rust test reads the names below and fails if they
// differ from the ones the core emits, so an event cannot be renamed, added or
// dropped on one side only. Listen through `onEvent`, never `listen` directly.
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { OverlayHealth } from './types';

export interface EventPayloads {
  'window-visibility': boolean;
  'user-data-imported': null;
  'update-channel-changed': null;
  'settings-updated': null;
  'open-spotlight': null;
  'overlay-health': OverlayHealth;
  /** A game id when only that game's stats changed, `null` when any may have. */
  'playtime-updated': string | null;
}

export type EventName = keyof EventPayloads;

/** Listen for a core event. Resolves to the function that stops listening; use it
 *  as `const p = onEvent(...); return () => { p.then((f) => f()); };`. */
export function onEvent<K extends EventName>(
  name: K,
  handler: (payload: EventPayloads[K]) => void,
): Promise<UnlistenFn> {
  return listen<EventPayloads[K]>(name, (e) => handler(e.payload));
}
