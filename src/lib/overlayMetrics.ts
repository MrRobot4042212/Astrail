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
  { key: 'show_frametime', tKey: 'metrics.frametime' },
  { key: 'show_gpu', tKey: 'metrics.gpuUsage' },
  { key: 'show_gpu_temp', tKey: 'metrics.gpuTemp' },
  { key: 'show_vram', tKey: 'metrics.vram' },
  { key: 'show_cpu', tKey: 'metrics.cpuUsage' },
  { key: 'show_cpu_temp', tKey: 'metrics.cpuTemp' },
  { key: 'show_ram', tKey: 'metrics.ram' },
];

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
  cpu_temp_c: 68,
};
