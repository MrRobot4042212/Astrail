// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

// What the settings screen tells the user about the privileged metrics. Pure, so
// the rule is tested once and the panel only maps the answer to strings.

import type { MetricsAccess } from './types';

/** FPS through PresentMon: an ETW session, granted to admins and to members of
 *  the built-in Performance Log Users group. */
export type FpsAccess = 'admin' | 'group' | 'blocked';

/** CPU temperature: the sidecar needs admin *and* the PawnIO driver, which
 *  Astrail never installs itself. */
export type CpuTempAccess = 'ok' | 'needsAdmin' | 'needsPawnio' | 'needsBoth';

export function fpsAccess(a: MetricsAccess): FpsAccess {
  if (a.elevated) return 'admin';
  return a.etw ? 'group' : 'blocked';
}

export function cpuTempAccess(a: MetricsAccess): CpuTempAccess {
  if (a.elevated && a.pawnio) return 'ok';
  if (a.elevated) return 'needsPawnio';
  return a.pawnio ? 'needsAdmin' : 'needsBoth';
}

/** Restarting as admin only helps when something is missing that admin gives. */
export function adminWouldHelp(a: MetricsAccess): boolean {
  return !a.elevated && (fpsAccess(a) === 'blocked' || cpuTempAccess(a) !== 'ok');
}
