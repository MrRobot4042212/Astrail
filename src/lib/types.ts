// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

// Every type that crosses the IPC boundary is generated from its Rust definition
// by ts-rs (`npm run bindings` writes `./bindings/`); never edit those by hand,
// and never hand-write a mirror of a Rust type here. What this file defines is
// either only on this side or not a Rust type at all.
import type { AppSettings } from './bindings/AppSettings';
import type { Game as GameModel } from './bindings/Game';

export type { AboutInfo } from './bindings/AboutInfo';
export type { AppError } from './bindings/AppError';
export type { AppSettings } from './bindings/AppSettings';
export type { AutostartState } from './bindings/AutostartState';
export type { BackupExportReport } from './bindings/BackupExportReport';
export type { BackupImportReport } from './bindings/BackupImportReport';
export type { BackupSummary } from './bindings/BackupSummary';
export type { Category } from './bindings/Category';
export type { CoverAnswer } from './bindings/CoverAnswer';
export type { DiskInfo } from './bindings/DiskInfo';
export type { DisplayInfo } from './bindings/DisplayInfo';
export type { ErrorCode } from './bindings/ErrorCode';
export type { GameSource } from './bindings/GameSource';
export type { GpuInfo } from './bindings/GpuInfo';
export type { GpuKind } from './bindings/GpuKind';
export type { HudFontSize } from './bindings/HudFontSize';
export type { MetricsAccess } from './bindings/MetricsAccess';
export type { MpoDiagnostics } from './bindings/MpoDiagnostics';
export type { MpoMode } from './bindings/MpoMode';
export type { OverlayPosition } from './bindings/OverlayPosition';
export type { OverlaySettings } from './bindings/OverlaySettings';
export type { PlayStat } from './bindings/PlayStat';
export type { Session } from './bindings/Session';
export type { SessionPerf } from './bindings/SessionPerf';
export type { ShortcutsSettings } from './bindings/ShortcutsSettings';
export type { SteamPlaytime } from './bindings/SteamPlaytime';
export type { SystemInfo } from './bindings/SystemInfo';
export type { UpdateChannel } from './bindings/UpdateChannel';
export type { UpdateOffer } from './bindings/UpdateOffer';

export type Game = GameModel & {
  /** Client-side only: extracted exe icon path (apps without cover/brand logo).
   *  Not part of the backend model; filled in lazily by `useLibrary`. */
  icon?: string;
};

/** Payload of the `overlay-health` event (`metrics::overlay_health`): 0 unknown,
 *  1 free (hardware plane), 2 costing (DWM composing). */
export type OverlayHealth = 0 | 1 | 2;

/** A partial `AppSettings` for `patchAppSettings`: nested objects merge key by key. */
export type AppSettingsPatch = {
  [K in keyof AppSettings]?: AppSettings[K] extends object ? Partial<AppSettings[K]> : AppSettings[K];
};

/** The legal documents embedded in the binary (the argument `legal_document`
 *  accepts; `about::document` rejects anything else). */
export type LegalDocument = 'license' | 'additional-terms' | 'third-party-notices';

/** One telemetry sample, for the settings-panel live preview (mock data) and the
 *  shared `OverlayPanel`. Not an IPC type: the HUD is drawn natively in Rust and
 *  no sample crosses to the webview any more. */
export interface MetricsSample {
  game?: string | null;
  /** Null on the first tick after a wake: CPU % needs two samples. */
  cpu_usage?: number | null;
  ram_used_mb: number;
  ram_total_mb: number;
  gpu_usage?: number | null;
  gpu_temp_c?: number | null;
  vram_used_mb?: number | null;
  vram_total_mb?: number | null;
  gpu_clock_mhz?: number | null;
  gpu_power_w?: number | null;
  /** CPU temperature from the LibreHardwareMonitor sidecar (admin + driver). */
  cpu_temp_c?: number | null;
  /** PresentMon (per swapchain) or the cputemp sidecar (AMD, fullscreen). Null when no fresh frame arrived. */
  fps?: number | null;
  /** PresentMon only; the sidecar reports an integer FPS, so no frametime is derived from it. */
  frametime_ms?: number | null;
  /** FPS of the 99th / 99.9th percentile frametime over the last 30 s. PresentMon only. */
  fps_low_1?: number | null;
  fps_low_01?: number | null;
  /** Worst frametime (ms) of each 200 ms slice, oldest first, at most 60. PresentMon only. */
  frametime_graph?: number[] | null;
  /** GPU work as a share (0-100) of the game's frame time over the last second.
   *  PresentMon only, and only while every frame carries a reading. */
  gpu_busy_pct?: number | null;
}
