// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

// How the settings screens print what `system_info` reports.

import type { GpuInfo } from './types';

/** MB → "16 GB" / "7.8 GB" / "512 MB". One decimal below 10 GB, where it still
 *  tells two cards apart; none above, where it is noise. */
export function fmtMem(mb: number): string {
  if (mb >= 1024) return `${(mb / 1024).toFixed(mb >= 10240 ? 0 : 1)} GB`;
  return `${mb} MB`;
}

/** Translation key for a GPU's kind. An unknown kind (`''`) has no entry, so
 *  the caller prints nothing instead of guessing. */
export const GPU_KIND_KEYS: Record<Exclude<GpuInfo['kind'], ''>, string> = {
  integrated: 'settings.sGpuIntegrated',
  discrete: 'settings.sGpuDiscrete',
};
