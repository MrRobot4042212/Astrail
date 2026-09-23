// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

// Types that cross the IPC boundary are generated from the Rust models by ts-rs
// (`cargo test` writes `./bindings/`); never edit them by hand. What follows in
// this file is either not generated yet or only exists on this side.
import type { AppSettings } from './bindings/AppSettings';
import type { Game as GameModel } from './bindings/Game';
import type { OverlaySettings } from './bindings/OverlaySettings';
import type { UpdateChannel } from './bindings/UpdateChannel';

export type { AppSettings } from './bindings/AppSettings';
export type { Category } from './bindings/Category';
export type { GameSource } from './bindings/GameSource';
export type { OverlaySettings } from './bindings/OverlaySettings';
export type { ShortcutsSettings } from './bindings/ShortcutsSettings';
export type { UpdateChannel } from './bindings/UpdateChannel';

export type Game = GameModel & {
  /** Client-side only: extracted exe icon path (apps without cover/brand logo).
   *  Not part of the backend model; filled in lazily by `useLibrary`. */
  icon?: string;
};

export type OverlayPosition = OverlaySettings['position'];

/** What the HUD measured during a session (`sessionperf.rs`). Every figure is
 *  optional: FPS needs PresentMon, temperatures need their sensor, and all of it
 *  is only measured while the HUD is on screen with the game in front. */
export interface SessionPerf {
  avg_fps?: number | null;
  /** 99th-percentile frame time of the whole session, as FPS. */
  low_1_fps?: number | null;
  /** Seconds of frames the FPS figures cover — not the session length. */
  fps_secs?: number | null;
  max_gpu_temp_c?: number | null;
  max_cpu_temp_c?: number | null;
}

/** One finished play session (unix timestamps, seconds). */
export interface Session {
  start: number;
  end: number;
  perf?: SessionPerf | null;
}

/** Accumulated play stats for a game. */
export interface PlayStat {
  seconds: number;
  last_played?: number | null;
  history: Session[];
}

/** What the Steam client recorded for an app (`steam_playtime.rs`). Steam's
 *  figure, across every machine of the account; never added to `PlayStat`. */
export interface SteamPlaytime {
  /** Minutes; never 0 (an unplayed app comes back as `null` instead). */
  minutes: number;
  /** Unix seconds. */
  last_played: number | null;
}

/** What `export_user_data` wrote (`backup.rs`). */
export interface BackupExportReport {
  path: string;
  covers: number;
  /** Covers left out because the file would have grown past its cap. */
  covers_skipped: number;
  /** Store files that exist but could not be read; they are not in the backup. */
  unreadable: string[];
}

/** What a picked backup holds, shown before the user confirms the import. Every
 *  value comes from the file, validated in Rust. */
export interface BackupSummary {
  file_name: string;
  /** Unix seconds; 0 when the file does not say. */
  created: number;
  app_version: string;
  manual_apps: number;
  played_games: number;
  favorites: number;
  hidden: number;
  categories: number;
  covers: number;
  includes_settings: boolean;
}

/** What `apply_user_data_backup` did. */
export interface BackupImportReport {
  covers: number;
  /** Where the data that was replaced went. */
  safety_copy: string;
}

/** One answer of `resolve_covers` (mirrors `art::Cover`). `unavailable` means
 *  nobody could be asked (offline, rate limited, no credentials): it says nothing
 *  about the game and must not be remembered as a miss. */
export type CoverAnswer =
  | { status: 'found'; path: string }
  | { status: 'not_found' }
  | { status: 'unavailable' };

/** Live overlay health: 0 unknown, 1 free (hardware plane), 2 costing (DWM composing). */
export type OverlayHealth = 0 | 1 | 2;

/**
 * Why the in-game overlay may be costing performance: live composition health plus the
 * system-config levers that decide whether Windows grants the HUD a hardware plane (MPO).
 */
export interface MpoDiagnostics {
  health: OverlayHealth;
  monitors: number;
  refresh_rates: number[];
  mixed_refresh: boolean;
  hags: boolean | null;
}

/** A partial `AppSettings` for `patchAppSettings`: nested objects merge key by key. */
export type AppSettingsPatch = {
  [K in keyof AppSettings]?: AppSettings[K] extends object ? Partial<AppSettings[K]> : AppSettings[K];
};

/**
 * What `check_update` returns: the updater plugin's own metadata (its `Update`
 * class is built from it) plus the channel the offer came from.
 */
export interface UpdateOffer {
  rid: number;
  currentVersion: string;
  version: string;
  date?: string;
  body?: string;
  rawJson: Record<string, unknown>;
  channel: UpdateChannel;
}

/** A GPU as reported by `system_info`. `key` is set only for metric-capable GPUs. */
export interface GpuInfo {
  name: string;
  vendor: string;
  vram_mb: number;
  /** Empty when Windows could not tell. */
  kind: 'integrated' | 'discrete' | '';
  key: string;
}

export interface DiskInfo {
  name: string;
  fs: string;
  total_mb: number;
  available_mb: number;
}

export interface DisplayInfo {
  name: string;
  width: number;
  height: number;
  refresh_hz: number;
  primary: boolean;
}

/** Mirror of `lib.rs::MetricsAccess`: what the privileged metrics can use. */
export interface MetricsAccess {
  /** Astrail runs as administrator. */
  elevated: boolean;
  /** PresentMon can open its ETW session: elevated or in Performance Log Users (FPS). */
  etw: boolean;
  /** The PawnIO driver is installed (CPU temperature, together with `elevated`). */
  pawnio: boolean;
}

/** Hardware/system info for the "Mi equipo" panel. */
export interface SystemInfo {
  cpu: string;
  cpu_cores: number;
  cpu_threads: number;
  ram_total_mb: number;
  os: string;
  motherboard: string | null;
  gpus: GpuInfo[];
  disks: DiskInfo[];
  displays: DisplayInfo[];
}

/** Version, author and license of the running build, for Settings → "Acerca de". */
export interface AboutInfo {
  version: string;
  author: string;
  repository: string;
  license: string;
  /** False in any build that did not come out of the project's own release workflow. */
  official: boolean;
}

/** The legal documents embedded in the binary. */
export type LegalDocument = 'license' | 'additional-terms' | 'third-party-notices';

/** One telemetry sample. The live HUD is now drawn natively in Rust; this type is
 *  kept for the settings-panel live preview (mock data) and the shared `OverlayPanel`. */
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

