'use client';

import { useEffect, useState } from 'react';
import { useTranslation, Trans } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { getAppSettings, patchAppSettings } from '@/lib/tauri';
import type { OverlaySettings, OverlayPosition } from '@/lib/types';
import { DEFAULT_SHORTCUTS, formatShortcut } from '@/lib/shortcuts';
import { CloseIcon } from './icons';
import { OverlayMpoPanel } from './OverlayMpoPanel';

const OVERLAY_POSITIONS: { value: OverlayPosition; tKey: string }[] = [
  { value: 'top-left', tKey: 'overlayScreen.posTopLeft' },
  { value: 'top-right', tKey: 'overlayScreen.posTopRight' },
  { value: 'bottom-left', tKey: 'overlayScreen.posBottomLeft' },
  { value: 'bottom-right', tKey: 'overlayScreen.posBottomRight' },
];

const OVERLAY_METRICS: { key: keyof OverlaySettings; tKey: string }[] = [
  { key: 'show_fps', tKey: 'metrics.fps' },
  { key: 'show_frametime', tKey: 'metrics.frametime' },
  { key: 'show_gpu', tKey: 'metrics.gpuUsage' },
  { key: 'show_gpu_temp', tKey: 'metrics.gpuTemp' },
  { key: 'show_vram', tKey: 'metrics.vram' },
  { key: 'show_cpu', tKey: 'metrics.cpuUsage' },
  { key: 'show_cpu_temp', tKey: 'metrics.cpuTemp' },
  { key: 'show_ram', tKey: 'metrics.ram' },
];

export function OverlaySettingsScreen({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const [overlay, setOverlay] = useState<OverlaySettings | null>(null);
  // Shortcut that returns to the game (overlay settings toggle); custom or default.
  const [settingsShortcut, setSettingsShortcut] = useState(DEFAULT_SHORTCUTS.overlay_settings);

  // Re-read on every settings change, so a change made in the launcher (or with
  // the overlay hotkey) while this screen is open is shown instead of reverted.
  useEffect(() => {
    const load = () =>
      getAppSettings()
        .then((s) => {
          setOverlay(s.overlay);
          if (s.shortcuts?.overlay_settings) {
            setSettingsShortcut(s.shortcuts.overlay_settings);
          }
        })
        .catch(() => {});
    load();
    const un = listen('settings-updated', load);
    un.catch(() => {});
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  function updateOverlay(patch: Partial<OverlaySettings>) {
    setOverlay((prev) => (prev ? { ...prev, ...patch } : prev));
    patchAppSettings({ overlay: patch }).catch(console.error);
  }

  // Handle Esc to close
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  if (!overlay) return null;

  return (
    <div className="fixed inset-0 z-[9999] flex items-center justify-center bg-void/70">
      <div className="relative w-full max-w-[600px] border border-line bg-surface p-7 shadow-card">
        <button
          onClick={onClose}
          className="absolute right-5 top-5 z-10 grid h-8 w-8 place-items-center text-muted transition hover:bg-elevated hover:text-ink"
          aria-label={t('common.close')}
        >
          <CloseIcon className="h-5 w-5" />
        </button>

        <h2 className="mb-1 font-display text-xl font-semibold text-ink">{t('overlayScreen.title')}</h2>
        <p className="mb-6 text-sm text-muted">
          <Trans
            i18nKey="overlayScreen.subtitle"
            values={{ shortcut: formatShortcut(settingsShortcut, t('common.keySpace')).join('+') }}
            components={{ k: <kbd className="bg-elevated px-1 font-mono text-xs" /> }}
          />
        </p>

        <div className="space-y-6">
          <div className="rounded border border-line bg-elevated/20 p-4">
            <h3 className="mb-3 text-sm font-medium text-ink">{t('overlayScreen.position')}</h3>
            <div className="flex overflow-hidden border border-line">
              {OVERLAY_POSITIONS.map((p) => (
                <button
                  key={p.value}
                  onClick={() => updateOverlay({ position: p.value })}
                  className={`flex-1 px-2 py-2 text-xs transition ${
                    overlay.position === p.value
                      ? 'bg-accent text-white'
                      : 'bg-elevated text-muted hover:text-ink'
                  }`}
                >
                  {t(p.tKey)}
                </button>
              ))}
            </div>
          </div>

          <OverlayMpoPanel
            mpoMode={overlay.mpo_mode}
            onModeChange={(m) => updateOverlay({ mpo_mode: m })}
          />

          <div className="rounded border border-line bg-elevated/20 p-4">
            <h3 className="mb-3 text-sm font-medium text-ink">{t('overlayScreen.metricsShow')}</h3>
            <div className="grid grid-cols-2 gap-1.5 sm:grid-cols-4">
              {OVERLAY_METRICS.map((m) => {
                const on = overlay[m.key] as boolean;
                return (
                  <button
                    key={m.key}
                    onClick={() =>
                      updateOverlay({ [m.key]: !on } as Partial<OverlaySettings>)
                    }
                    className={`flex items-center justify-between border px-2.5 py-2 text-xs transition ${
                      on ? 'border-accent/40 bg-elevated text-ink' : 'border-line text-muted'
                    }`}
                  >
                    <span>{t(m.tKey)}</span>
                    <span
                      className={`h-2.5 w-2.5 border ${
                        on ? 'border-accent bg-accent' : 'border-muted'
                      }`}
                    />
                  </button>
                );
              })}
            </div>
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div className="rounded border border-line bg-elevated/20 p-4">
              <h3 className="mb-3 text-sm font-medium text-ink">{t('overlayScreen.textSize')}</h3>
              <div className="flex overflow-hidden border border-line">
                {[
                  { label: t('overlayScreen.sizeSmall'), value: 'xs' },
                  { label: t('overlayScreen.sizeNormal'), value: 'sm' },
                  { label: t('overlayScreen.sizeLarge'), value: 'base' },
                ].map((o) => (
                  <button
                    key={o.value}
                    onClick={() => updateOverlay({ font_size: o.value })}
                    className={`flex-1 px-2 py-2 text-xs transition ${
                      overlay.font_size === o.value
                        ? 'bg-accent font-medium text-white'
                        : 'bg-elevated text-muted hover:text-ink'
                    }`}
                  >
                    {o.label}
                  </button>
                ))}
              </div>
            </div>

            <div className="rounded border border-line bg-elevated/20 p-4">
              <h3 className="mb-3 text-sm font-medium text-ink">{t('overlayScreen.background')}</h3>
              <div className="flex overflow-hidden border border-line">
                {[
                  { label: '50%', value: 50 },
                  { label: '70%', value: 70 },
                  { label: '85%', value: 85 },
                  { label: '95%', value: 95 },
                ].map((o) => (
                  <button
                    key={o.value}
                    onClick={() => updateOverlay({ bg_opacity: o.value as number })}
                    className={`flex-1 px-2 py-2 text-xs transition ${
                      overlay.bg_opacity === o.value
                        ? 'bg-accent font-medium text-white'
                        : 'bg-elevated text-muted hover:text-ink'
                    }`}
                  >
                    {o.label}
                  </button>
                ))}
              </div>
            </div>
          </div>

        </div>
      </div>
    </div>
  );
}
