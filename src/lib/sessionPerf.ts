// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import type { Session, SessionPerf } from './types';

/** Rows shown in the detail view. The history holds up to 500 sessions. */
export const SESSION_PERF_ROWS = 5;

/** Placeholder for a figure that was not measured. Never a zero: `0 °C` and
 *  `0 FPS` read as measurements. */
export const NOT_MEASURED = '—';

export interface MeasuredSession {
  start: number;
  end: number;
  perf: SessionPerf;
}

function isFigure(v: number | null | undefined): v is number {
  return typeof v === 'number' && Number.isFinite(v) && v > 0;
}

/** True when the summary holds at least one figure worth a row. */
export function hasFigures(perf: SessionPerf | null | undefined): perf is SessionPerf {
  if (!perf) return false;
  return (
    isFigure(perf.avg_fps) ||
    isFigure(perf.low_1_fps) ||
    isFigure(perf.max_gpu_temp_c) ||
    isFigure(perf.max_cpu_temp_c)
  );
}

/** The newest `limit` sessions in which something was measured, newest first.
 *  The history is stored oldest first. */
export function measuredSessions(
  history: readonly Session[],
  limit: number = SESSION_PERF_ROWS,
): MeasuredSession[] {
  const out: MeasuredSession[] = [];
  for (let i = history.length - 1; i >= 0 && out.length < limit; i--) {
    const s = history[i];
    if (hasFigures(s.perf)) out.push({ start: s.start, end: s.end, perf: s.perf });
  }
  return out;
}

/** One decimal, as stored. Rounding 59.4 to "59" would hide the difference
 *  between a locked 60 and a game that does not hold it. */
export function formatFps(v: number | null | undefined): string {
  return isFigure(v) ? v.toFixed(1) : NOT_MEASURED;
}

export function formatTemp(v: number | null | undefined): string {
  return isFigure(v) ? `${Math.round(v)} °C` : NOT_MEASURED;
}

/** A span of seconds as `45 s`, `32 min` or `1 h 5 min`. */
export function formatSpan(seconds: number | null | undefined): string {
  if (!isFigure(seconds)) return NOT_MEASURED;
  const total = Math.floor(seconds);
  if (total < 60) return `${total} s`;
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  if (h === 0) return `${m} min`;
  return m === 0 ? `${h} h` : `${h} h ${m} min`;
}
