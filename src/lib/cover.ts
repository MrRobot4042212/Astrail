// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { convertFileSrc } from '@tauri-apps/api/core';

/** Turn a cover value into something an <img> can load.
 *  - Remote URLs (manual overrides) are used as-is.
 *  - Local cached file paths (from IGDB caching) go through the Tauri asset
 *    protocol so the webview can read them off disk. */
export function coverSrc(value?: string | null): string | undefined {
  if (!value) return undefined;
  if (/^https?:\/\//i.test(value)) return value;
  return convertFileSrc(value);
}
