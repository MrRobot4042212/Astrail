// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { describe, expect, it } from 'vitest';

import type { Session } from './types';
import {
  NOT_MEASURED,
  formatFps,
  formatSpan,
  formatTemp,
  hasFigures,
  measuredSessions,
} from './sessionPerf';

describe('measured sessions', () => {
  it('skips sessions nothing was measured in, newest first', () => {
    const history: Session[] = [
      { start: 10, end: 20, perf: { avg_fps: 60, fps_secs: 40 } },
      { start: 30, end: 40 },
      { start: 50, end: 60, perf: null },
      { start: 70, end: 80, perf: {} },
      { start: 90, end: 100, perf: { max_gpu_temp_c: 71 } },
    ];
    expect(measuredSessions(history).map((s) => s.start)).toEqual([90, 10]);
  });

  it('keeps only the newest rows', () => {
    const history: Session[] = Array.from({ length: 12 }, (_, i) => ({
      start: i * 100,
      end: i * 100 + 50,
      perf: { avg_fps: 100 + i },
    }));
    const rows = measuredSessions(history, 5);
    expect(rows.map((s) => s.start)).toEqual([1100, 1000, 900, 800, 700]);
  });

  it('does not count fps_secs alone, or a zero, as a figure', () => {
    expect(hasFigures({ fps_secs: 120 })).toBe(false);
    expect(hasFigures({ avg_fps: 0, max_cpu_temp_c: 0 })).toBe(false);
    expect(hasFigures({ max_cpu_temp_c: 64 })).toBe(true);
    expect(hasFigures(undefined)).toBe(false);
  });
});

describe('figures', () => {
  it('never shows an unmeasured figure as a zero', () => {
    expect(formatFps(null)).toBe(NOT_MEASURED);
    expect(formatFps(undefined)).toBe(NOT_MEASURED);
    expect(formatFps(0)).toBe(NOT_MEASURED);
    expect(formatFps(Number.NaN)).toBe(NOT_MEASURED);
    expect(formatTemp(null)).toBe(NOT_MEASURED);
    expect(formatTemp(0)).toBe(NOT_MEASURED);
    expect(formatSpan(undefined)).toBe(NOT_MEASURED);
  });

  it('keeps the stored decimal on FPS', () => {
    expect(formatFps(59.4)).toBe('59.4');
    expect(formatFps(143.3)).toBe('143.3');
    expect(formatFps(60)).toBe('60.0');
    expect(formatTemp(74)).toBe('74 °C');
  });

  it('formats spans in the unit that fits', () => {
    expect(formatSpan(45)).toBe('45 s');
    expect(formatSpan(60)).toBe('1 min');
    expect(formatSpan(1920)).toBe('32 min');
    expect(formatSpan(3600)).toBe('1 h');
    expect(formatSpan(3900)).toBe('1 h 5 min');
  });
});
