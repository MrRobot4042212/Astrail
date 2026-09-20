// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import type { MetricsSample, OverlaySettings } from './types';

/** The boolean `show_*` switches of the HUD, in the order the HUD draws them. */
export type OverlayMetricKey = {
  [K in keyof OverlaySettings]: OverlaySettings[K] extends boolean ? K : never;
}[keyof OverlaySettings];

/** One list for the launcher settings and the in-game screen, so they cannot drift. */
export const OVERLAY_METRICS: { key: OverlayMetricKey; tKey: string }[] = [
  { key: 'show_fps', tKey: 'metrics.fps' },
  { key: 'show_lows', tKey: 'metrics.lows' },
  { key: 'show_frametime', tKey: 'metrics.frametime' },
  { key: 'show_frametime_graph', tKey: 'metrics.frametimeGraph' },
  { key: 'show_gpu', tKey: 'metrics.gpuUsage' },
  { key: 'show_gpu_temp', tKey: 'metrics.gpuTemp' },
  { key: 'show_vram', tKey: 'metrics.vram' },
  { key: 'show_cpu', tKey: 'metrics.cpuUsage' },
  { key: 'show_cpu_temp', tKey: 'metrics.cpuTemp' },
  { key: 'show_ram', tKey: 'metrics.ram' },
];

/** Slices the native HUD keeps (`presentmon::GRAPH_POINTS`). */
export const GRAPH_POINTS = 60;

/** A steady ~144 fps run with two hitches, so the preview shows what a spike looks like. */
const PREVIEW_GRAPH: number[] = Array.from({ length: GRAPH_POINTS }, (_, i) => {
  if (i === 21) return 16.8;
  if (i === 47) return 11.9;
  return 6.9 + ((i * 7) % 5) * 0.15;
});

/** Sample telemetry for the HUD preview in the settings panel. */
export const PREVIEW_SAMPLE: MetricsSample = {
  game: 'Cyberpunk 2077',
  cpu_usage: 34,
  ram_used_mb: 16384,
  ram_total_mb: 32768,
  gpu_usage: 87,
  gpu_temp_c: 72,
  vram_used_mb: 8192,
  vram_total_mb: 12288,
  fps: 144,
  frametime_ms: 6.9,
  fps_low_1: 112,
  fps_low_01: 84,
  frametime_graph: PREVIEW_GRAPH,
  cpu_temp_c: 68,
};

/** A bar never disappears: a very fast slice still leaves a mark. */
const GRAPH_MIN_BAR = 0.06;

/** What the graph is scaled against: the median of its points (`overlay::graph_baseline_ms`). */
export function graphBaseline(points: readonly number[]): number {
  if (points.length === 0) return 0;
  const sorted = [...points].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

/** Same rule as the native HUD (`overlay::graph_bar`): full height is twice the
 *  baseline, and anything over 1.5× the baseline is a spike. */
export function graphBar(frametimeMs: number, baselineMs: number): { height: number; spike: boolean } {
  if (baselineMs <= 0 || !Number.isFinite(frametimeMs)) {
    return { height: GRAPH_MIN_BAR, spike: false };
  }
  const ratio = frametimeMs / baselineMs;
  return { height: Math.min(1, Math.max(GRAPH_MIN_BAR, ratio / 2)), spike: ratio > 1.5 };
}
