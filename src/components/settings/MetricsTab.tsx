// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useTranslation, Trans } from 'react-i18next';
import type { HudFontSize, OverlayPosition, OverlaySettings } from '@/lib/types';
import type { SettingsModel } from '@/hooks/useSettingsModel';
import { DEFAULT_SHORTCUTS, formatShortcut } from '@/lib/shortcuts';
import { OVERLAY_METRICS, PREVIEW_SAMPLE } from '@/lib/overlayMetrics';
import { GPU_KIND_KEYS } from '@/lib/systemFormat';
import { OverlayPanel } from '../Overlay';
import { OverlayMpoPanel } from '../OverlayMpoPanel';
import { ColorPicker } from './ColorPicker';
import { MetricsAccessPanel } from './MetricsAccessPanel';
import { Card, SegmentedControl, TabHeader, Toggle } from './primitives';

const OVERLAY_POSITIONS: { value: OverlayPosition; tKey: string }[] = [
  { value: 'top-left', tKey: 'overlayScreen.posTopLeft' },
  { value: 'top-right', tKey: 'overlayScreen.posTopRight' },
  { value: 'bottom-left', tKey: 'overlayScreen.posBottomLeft' },
  { value: 'bottom-right', tKey: 'overlayScreen.posBottomRight' },
];

export function MetricsTab({ model }: { model: SettingsModel }) {
  const { sys, overlay, updateOverlay, access, restartAdmin, shortcuts } = model;
  const { t } = useTranslation();
  const toggleKey = formatShortcut(shortcuts?.overlay_toggle ?? DEFAULT_SHORTCUTS.overlay_toggle, t('common.keySpace')).join('+');
  return (
    <div>
      <TabHeader
        title={t('settings.tabMetrics')}
        desc={t('settings.mDesc')}
      />
      {!overlay ? (
        <p className="text-sm text-muted">{t('settings.loading')}</p>
      ) : (
        <div className="space-y-6">
          {access && (
            <MetricsAccessPanel
              access={access}
              cpuTempWanted={overlay.show_cpu_temp}
              onRestartAdmin={restartAdmin}
            />
          )}
          <Card
            title={t('settings.mOverlayTitle')}
            control={
              <Toggle
                on={overlay.enabled}
                onClick={() => updateOverlay({ enabled: !overlay.enabled })}
                label={t('settings.mOverlayTitle')}
              />
            }
          >
            <p className="text-xs leading-relaxed text-muted">
              <Trans
                i18nKey="settings.mOverlayBody"
                values={{ key: toggleKey }}
                components={[<kbd key="0" className="bg-elevated px-1" />]}
              />
            </p>
            <p className="mt-2 text-[11px] leading-relaxed text-muted/70">
              <Trans i18nKey="settings.mOverlayWarn" components={[<strong key="0" />]} />
            </p>
          </Card>

          {overlay.enabled && (
            <>
              <Card title={t('settings.mPosition')}>
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
              </Card>

              <OverlayMpoPanel
                mpoMode={overlay.mpo_mode}
                onModeChange={(m) => updateOverlay({ mpo_mode: m })}
              />

              {sys && sys.gpus.some((g) => g.key) && (
                <Card title={t('settings.mGpuCard')}>
                  <select
                    value={overlay.gpu}
                    onChange={(e) => updateOverlay({ gpu: e.target.value })}
                    className="w-full border border-line bg-elevated px-3 py-2 text-sm text-ink outline-none focus:border-accent"
                  >
                    <option value="auto">{t('settings.mGpuAuto')}</option>
                    {sys.gpus
                      .filter((g) => g.key)
                      .map((g) => (
                        <option key={g.key} value={g.key}>
                          {g.name}
                          {g.kind ? ` · ${t(GPU_KIND_KEYS[g.kind])}` : ''}
                        </option>
                      ))}
                  </select>
                </Card>
              )}

              <Card title={t('settings.mMetricsShow')}>
                <div className="grid grid-cols-2 gap-1.5 sm:grid-cols-3">
                  {OVERLAY_METRICS.map((m) => {
                    const on = overlay[m.key];
                    return (
                      <button
                        key={m.key}
                        onClick={() => updateOverlay({ [m.key]: !on })}
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
              </Card>

              {/* ── Appearance ─────────────────────────────────────────────── */}
              <Card title={t('settings.mAppearance')}>
                <div className="space-y-5">
                  {/* Color pickers */}
                  <div>
                    <p className="mb-2.5 text-[11px] font-medium uppercase tracking-wide text-muted">{t('settings.mColors')}</p>
                    <div className="space-y-2">
                      <ColorPicker
                        label={t('settings.mLabelsColor')}
                        value={overlay.label_color}
                        onChange={(v) => updateOverlay({ label_color: v })}
                      />
                      <ColorPicker
                        label={t('settings.mValuesColor')}
                        value={overlay.value_color}
                        onChange={(v) => updateOverlay({ value_color: v })}
                      />
                      <ColorPicker
                        label={t('settings.mAccentColor')}
                        value={overlay.accent_color}
                        onChange={(v) => updateOverlay({ accent_color: v })}
                      />
                    </div>
                  </div>

                  {/* Background opacity */}
                  <div>
                    <p className="mb-2.5 text-[11px] font-medium uppercase tracking-wide text-muted">{t('settings.mBgOpacity')}</p>
                    <SegmentedControl
                      options={[
                        { label: '50%', value: 50 },
                        { label: '70%', value: 70 },
                        { label: '85%', value: 85 },
                        { label: '95%', value: 95 },
                      ]}
                      value={overlay.bg_opacity}
                      onChange={(v) => updateOverlay({ bg_opacity: v as number })}
                    />
                  </div>

                  {/* Font size */}
                  <div>
                    <p className="mb-2.5 text-[11px] font-medium uppercase tracking-wide text-muted">{t('settings.mTextSize')}</p>
                    <SegmentedControl<HudFontSize>
                      options={[
                        { label: t('settings.mSizeSmall'), value: 'xs' },
                        { label: t('settings.mSizeNormal'), value: 'sm' },
                        { label: t('settings.mSizeLarge'), value: 'base' },
                      ]}
                      value={overlay.font_size}
                      onChange={(v) => updateOverlay({ font_size: v })}
                    />
                  </div>
                </div>
              </Card>

              {/* ── Live preview ───────────────────────────────────────────── */}
              <Card title={t('settings.mPreview')}>
                <p className="mb-3 text-xs text-muted">{t('settings.mPreviewNote')}</p>
                <OverlayPreview cfg={overlay} />
              </Card>
            </>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * Live preview of the overlay inside the settings panel.
 * Uses mock sample data so the user can see exactly how the HUD will look.
 */
function OverlayPreview({ cfg }: { cfg: OverlaySettings }) {
  const { t } = useTranslation();
  const isTop  = cfg.position.startsWith('top');
  const isLeft = cfg.position.endsWith('left');

  return (
    <div className="relative overflow-hidden border border-line" style={{ aspectRatio: '16/9' }}>
      {/* Simulated game background */}
      <div className="absolute inset-0 bg-gradient-to-br from-gray-950 via-slate-900 to-gray-950" />
      {/* Subtle scanline texture */}
      <div
        className="absolute inset-0 opacity-10"
        style={{
          backgroundImage: 'repeating-linear-gradient(0deg, transparent, transparent 3px, rgba(0,0,0,0.4) 3px, rgba(0,0,0,0.4) 4px)',
        }}
      />
      {/* Faint game-world decoration */}
      <div className="absolute inset-0 flex items-center justify-center">
        <span className="select-none text-[11px] font-medium uppercase tracking-widest text-white/10">
          {t('settings.mPreviewWord')}
        </span>
      </div>
      {/* Overlay panel positioned in the chosen corner */}
      <div
        className="absolute"
        style={{
          top:    isTop    ? '8px'  : undefined,
          bottom: !isTop   ? '8px'  : undefined,
          left:   isLeft   ? '8px'  : undefined,
          right:  !isLeft  ? '8px'  : undefined,
        }}
      >
        {/* Scale down the panel so it fits nicely in the preview box */}
        <div style={{ transform: 'scale(0.85)', transformOrigin: isTop ? (isLeft ? 'top left' : 'top right') : (isLeft ? 'bottom left' : 'bottom right') }}>
          <OverlayPanel cfg={cfg} sample={PREVIEW_SAMPLE} />
        </div>
      </div>
    </div>
  );
}
