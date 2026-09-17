'use client';

import { useEffect } from 'react';
import { setOverlayInteractive } from '@/lib/tauri';
import type { MetricsSample, OverlaySettings } from '@/lib/types';
import { OverlaySettingsScreen } from './OverlaySettingsScreen';

/** Corner placement → fixed-position classes. */
export const OVERLAY_CORNER: Record<string, string> = {
  'top-left': 'top-3 left-3',
  'top-right': 'top-3 right-3',
  'bottom-left': 'bottom-3 left-3',
  'bottom-right': 'bottom-3 right-3',
};

/** Font-size key → label/value Tailwind text-size classes. */
const FONT_SIZE_MAP = {
  xs:   { label: 'text-[9px]',  value: 'text-xs'   },
  sm:   { label: 'text-[10px]', value: 'text-sm'   },
  base: { label: 'text-xs',     value: 'text-base' },
} as const;

/** Dynamic temperature color (CSS hex, not Tailwind — keeps it compatible with inline styles). */
function tempCssColor(c: number): string {
  if (c >= 85) return '#f87171'; // red-400  — danger
  if (c >= 75) return '#fbbf24'; // amber-400 — hot
  return '#34d399';               // emerald-400 — cool
}

/** One metric line in the HUD. Uses inline colors from the overlay config. */
function Row({
  label,
  value,
  labelColor,
  valueColor,
  labelCls,
  valueCls,
}: {
  label: string;
  value: string;
  labelColor: string;
  valueColor: string;
  labelCls: string;
  valueCls: string;
}) {
  return (
    <div className="flex items-baseline justify-between gap-4 leading-tight">
      <span className={`${labelCls} uppercase tracking-wider`} style={{ color: labelColor }}>
        {label}
      </span>
      <span className={`${valueCls} font-semibold tabular-nums`} style={{ color: valueColor }}>
        {value}
      </span>
    </div>
  );
}

/**
 * Core HUD panel — renders the overlay with the given config and sample data.
 * Exported so `SettingsDialog` can reuse it with mock data for the live preview.
 */
export function OverlayPanel({
  cfg,
  sample,
}: {
  cfg: OverlaySettings;
  sample: MetricsSample;
}) {
  const gb = (mb: number) => (mb / 1024).toFixed(1);
  const sizeKey = (cfg.font_size ?? 'sm') as keyof typeof FONT_SIZE_MAP;
  const { label: labelCls, value: valueCls } =
    FONT_SIZE_MAP[sizeKey] ?? FONT_SIZE_MAP.sm;

  const labelColor  = cfg.label_color  ?? '#8a8a8a';
  const valueColor  = cfg.value_color  ?? '#ffffff';
  const accentColor = cfg.accent_color ?? '#ef4444';
  const bgOpacity   = (cfg.bg_opacity  ?? 85) / 100;

  const rows: { label: string; value: string; color: string }[] = [];

  if (cfg.show_fps && sample.fps != null) {
    rows.push({ label: 'FPS', value: sample.fps.toFixed(0), color: accentColor });
  }
  if (cfg.show_frametime && sample.frametime_ms != null) {
    rows.push({ label: 'Frame', value: `${sample.frametime_ms.toFixed(1)} ms`, color: valueColor });
  }
  if (cfg.show_gpu && sample.gpu_usage != null) {
    rows.push({ label: 'GPU', value: `${sample.gpu_usage}%`, color: accentColor });
  }
  if (cfg.show_gpu_temp && sample.gpu_temp_c != null) {
    rows.push({ label: 'GPU °C', value: `${sample.gpu_temp_c}°`, color: tempCssColor(sample.gpu_temp_c) });
  }
  if (cfg.show_vram && sample.vram_used_mb != null && sample.vram_total_mb != null) {
    rows.push({ label: 'VRAM', value: `${gb(sample.vram_used_mb)}/${gb(sample.vram_total_mb)} GB`, color: valueColor });
  }
  if (cfg.show_cpu && sample.cpu_usage != null) {
    rows.push({ label: 'CPU', value: `${sample.cpu_usage.toFixed(0)}%`, color: accentColor });
  }
  if (cfg.show_cpu_temp && sample.cpu_temp_c != null) {
    rows.push({ label: 'CPU °C', value: `${sample.cpu_temp_c}°`, color: tempCssColor(sample.cpu_temp_c) });
  }
  if (cfg.show_ram) {
    rows.push({ label: 'RAM', value: `${gb(sample.ram_used_mb)}/${gb(sample.ram_total_mb)} GB`, color: valueColor });
  }

  if (rows.length === 0) return null;

  return (
    <div
      className="min-w-[148px] border border-white/10 px-3 py-2 font-mono"
      style={{ background: `rgba(0,0,0,${bgOpacity})` }}
    >
      {sample.game && (
        <div
          className="mb-1.5 max-w-[180px] truncate border-b border-white/10 pb-1 text-[10px] font-semibold uppercase tracking-wider"
          style={{ color: accentColor }}
        >
          {sample.game}
        </div>
      )}
      <div className="space-y-0.5">
        {rows.map((r) => (
          <Row
            key={r.label}
            label={r.label}
            value={r.value}
            labelColor={labelColor}
            valueColor={r.color}
            labelCls={labelCls}
            valueCls={valueCls}
          />
        ))}
      </div>
    </div>
  );
}

/**
 * Host for the in-game overlay *settings* screen. The HUD itself is drawn by a native
 * layered window from Rust (`overlay_dcomp`/`overlay_native`) for minimal latency, so
 * this WebView2 window's *only* job is the settings screen. It is now created on demand
 * by Rust (`ensure_overlay_window`) when the settings hotkey opens it and destroyed on
 * close, so no WebView2/Chromium process sits resident during gameplay. Because the
 * window exists only while settings are open, we render the screen immediately on mount;
 * closing it tells Rust to destroy the window.
 */
export function Overlay() {
  useEffect(() => {
    // The overlay window must be see-through (globals.css paints an opaque bg).
    document.documentElement.style.background = 'transparent';
    document.body.style.background = 'transparent';
  }, []);

  return (
    <OverlaySettingsScreen
      onClose={() => {
        setOverlayInteractive(false).catch(() => {});
      }}
    />
  );
}
