export type GameSource =
  | 'steam'
  | 'epic'
  | 'gog'
  | 'ea'
  | 'ubisoft'
  | 'xbox'
  | 'battlenet'
  | 'riot'
  | 'rockstar'
  | 'amazon'
  | 'battlestate'
  | 'windows'
  | 'app'
  | 'manual';

/** A user-created category with an optional bundled-icon key. */
export interface Category {
  name: string;
  icon?: string | null;
}


/** One finished play session (unix timestamps, seconds). */
export interface Session {
  start: number;
  end: number;
}

/** Accumulated play stats for a game. */
export interface PlayStat {
  seconds: number;
  last_played?: number | null;
  history: Session[];
}

export interface Game {
  id: string;
  name: string;
  source: GameSource;
  app_id?: number | null;
  executable?: string | null;
  install_dir?: string | null;
  cover_url?: string | null;
  launch_uri?: string | null;
  /** User overlay: marked as favorite. */
  favorite?: boolean;
  /** User overlay: manual categories assigned to this entry. */
  categories?: string[];
  /** Client-side only: extracted exe icon path (apps without cover/brand logo).
   *  Not part of the backend model; filled in lazily by `useLibrary`. */
  icon?: string;
}

export type OverlayPosition = 'top-left' | 'top-right' | 'bottom-left' | 'bottom-right';

/** In-game metrics overlay configuration (part of AppSettings). */
export interface OverlaySettings {
  enabled: boolean;
  position: OverlayPosition;
  interval_ms: number;
  show_fps: boolean;
  show_frametime: boolean;
  show_gpu: boolean;
  show_gpu_temp: boolean;
  show_vram: boolean;
  show_cpu: boolean;
  show_cpu_temp: boolean;
  show_ram: boolean;
  /** Which GPU to sample: "auto" | "nvml:<i>" | "pci:<AMD PnP id fragment>". */
  gpu: string;
  /** CSS hex color for metric labels ("FPS", "GPU"…). */
  label_color: string;
  /** CSS hex color for metric values (numbers). */
  value_color: string;
  /** CSS hex color for accented values (FPS, GPU%, CPU%) and the game title. */
  accent_color: string;
  /** HUD background opacity 0–100. */
  bg_opacity: number;
  /** Font size key: "xs" | "sm" | "base". */
  font_size: string;
  /**
   * What to do when the HUD can't get a hardware overlay plane and DWM composites it
   * (the game loses independent-flip → FPS/latency cost): "always" (draw regardless) |
   * "performance" (auto-hide the HUD once a stable composed state is detected).
   */
  mpo_mode: "always" | "performance";
}

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

export interface ShortcutsSettings {
  spotlight: string;
  overlay_toggle: string;
  overlay_settings: string;
}

/** A partial `AppSettings` for `patchAppSettings`: nested objects merge key by key. */
export type AppSettingsPatch = {
  [K in keyof AppSettings]?: AppSettings[K] extends object ? Partial<AppSettings[K]> : AppSettings[K];
};

export interface AppSettings {
  setup_completed: boolean;
  minimize_to_tray: boolean;
  overlay: OverlaySettings;
  shortcuts: ShortcutsSettings;
  /** UI language: "system" (follow OS, fallback English), "es" or "en". */
  language: string;
  /**
   * Publish the game being played to Discord Rich Presence. Opt-in: this
   * broadcasts what you are playing to your whole friends list.
   */
  discord_enabled: boolean;
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
}

