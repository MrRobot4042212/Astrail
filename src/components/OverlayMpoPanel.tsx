'use client';

import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { overlayMpoDiagnostics } from '@/lib/tauri';
import type { MpoDiagnostics, OverlayHealth } from '@/lib/types';

type MpoMode = 'always' | 'performance';

/**
 * Overlay performance (MPO) panel: shows the live composition health of the in-game HUD
 * — free (on a hardware overlay plane) vs costing (DWM is compositing it → the game
 * loses independent-flip) — lets the user pick what to do when it costs FPS, and, when
 * costing, surfaces the system-config blockers (multi-monitor, mixed refresh, HAGS) that
 * keep Windows from granting a hardware plane. Shared by the in-game settings screen and
 * the main settings dialog.
 */
export function OverlayMpoPanel({
  mpoMode,
  onModeChange,
}: {
  mpoMode: MpoMode;
  onModeChange: (m: MpoMode) => void;
}) {
  const { t } = useTranslation();
  const [diag, setDiag] = useState<MpoDiagnostics | null>(null);
  const [health, setHealth] = useState<OverlayHealth>(0);

  // Live health updates from the sampler.
  useEffect(() => {
    const un = listen<OverlayHealth>('overlay-health', (e) => setHealth(e.payload));
    un.catch(() => {});
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  // Pull the diagnostics on open and whenever health flips (picks up monitor
  // changes). `pulledFor` stops the health taken from a pull from triggering a
  // second, identical pull.
  const pulledFor = useRef<OverlayHealth | null>(null);
  useEffect(() => {
    if (pulledFor.current === health) return;
    let cancelled = false;
    overlayMpoDiagnostics()
      .then((d) => {
        if (cancelled) return;
        pulledFor.current = d.health;
        setDiag(d);
        setHealth(d.health);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [health]);

  const healthLabel =
    health === 1
      ? t('overlayMpo.healthFree')
      : health === 2
        ? t('overlayMpo.healthCosting')
        : t('overlayMpo.healthUnknown');
  const healthColor =
    health === 1
      ? 'border-info text-info'
      : health === 2
        ? 'border-destructive text-destructive'
        : 'border-muted text-muted';

  const tips: string[] = [];
  if (diag) {
    if (diag.monitors > 1) tips.push(t('overlayMpo.tipMultimonitor', { n: diag.monitors }));
    if (diag.mixed_refresh)
      tips.push(t('overlayMpo.tipMixedRefresh', { rates: `${diag.refresh_rates.join(' / ')} Hz` }));
    if (diag.hags === false) tips.push(t('overlayMpo.tipHags'));
  }

  return (
    <div className="rounded border border-line bg-elevated/20 p-4">
      <div className="mb-3 flex items-center justify-between gap-2">
        <h3 className="text-sm font-medium text-ink">{t('overlayMpo.title')}</h3>
        <span className={`border px-2 py-0.5 text-[11px] font-medium ${healthColor}`}>
          {healthLabel}
        </span>
      </div>

      <div className="flex overflow-hidden border border-line">
        {(
          [
            { value: 'always', label: t('overlayMpo.modeAlways') },
            { value: 'performance', label: t('overlayMpo.modePerformance') },
          ] as const
        ).map((o) => (
          <button
            key={o.value}
            onClick={() => onModeChange(o.value)}
            className={`flex-1 px-2 py-2 text-xs transition ${
              mpoMode === o.value ? 'bg-accent text-white' : 'bg-elevated text-muted hover:text-ink'
            }`}
          >
            {o.label}
          </button>
        ))}
      </div>
      <p className="mt-2 text-[11px] leading-relaxed text-muted">
        {mpoMode === 'performance'
          ? t('overlayMpo.modePerformanceHint')
          : t('overlayMpo.modeAlwaysHint')}
      </p>

      {health === 2 && tips.length > 0 && (
        <div className="mt-3 border border-destructive/30 bg-destructive/5 p-3">
          <p className="mb-1.5 text-xs font-medium text-destructive">{t('overlayMpo.fixTitle')}</p>
          <ul className="space-y-1 text-[11px] leading-relaxed text-muted">
            {tips.map((tip, i) => (
              <li key={i}>• {tip}</li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
