// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useEffect, useRef, useState } from 'react';
import { useTranslation, Trans } from 'react-i18next';
import {
  clearCoverCache,
  hiddenCount,
  restoreHidden,
  getDiscordClientId,
  setDiscordClientId,
  getAutostart,
  setAutostart,
  getAppSettings,
  patchAppSettings,
  systemInfo,
  isElevated,
  restartAsAdmin,
} from '@/lib/tauri';
import type { GpuInfo, OverlaySettings, OverlayPosition, SystemInfo, MetricsSample, ShortcutsSettings } from '@/lib/types';
import { CloseIcon, InfoIcon, GearIcon, FireIcon } from './icons';
import { DEFAULT_SHORTCUTS, formatShortcut, recordShortcut } from '@/lib/shortcuts';
import { colorToCommit } from '@/lib/color';
import { OverlayPanel } from './Overlay';
import { OverlayMpoPanel } from './OverlayMpoPanel';

/** Sample telemetry data used by the live overlay preview inside the settings panel. */
const PREVIEW_SAMPLE: MetricsSample = {
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

/** MB → human GB/MB string. */
function fmtMem(mb: number): string {
  if (mb >= 1024) return `${(mb / 1024).toFixed(mb >= 10240 ? 0 : 1)} GB`;
  return `${mb} MB`;
}

const GPU_KIND_KEYS: Record<Exclude<GpuInfo['kind'], ''>, string> = {
  integrated: 'settings.sGpuIntegrated',
  discrete: 'settings.sGpuDiscrete',
};

const OVERLAY_POSITIONS: { value: OverlayPosition; tKey: string }[] = [
  { value: 'top-left', tKey: 'overlayScreen.posTopLeft' },
  { value: 'top-right', tKey: 'overlayScreen.posTopRight' },
  { value: 'bottom-left', tKey: 'overlayScreen.posBottomLeft' },
  { value: 'bottom-right', tKey: 'overlayScreen.posBottomRight' },
];

// Metric toggles shown in the overlay settings. FPS/frametime depend on the
// PresentMon integration (later phase); flagged so expectations are clear.
const OVERLAY_METRICS: { key: keyof OverlaySettings; tKey: string; note?: string }[] = [
  { key: 'show_fps', tKey: 'metrics.fps' },
  { key: 'show_frametime', tKey: 'metrics.frametime' },
  { key: 'show_gpu', tKey: 'metrics.gpuUsage' },
  { key: 'show_gpu_temp', tKey: 'metrics.gpuTemp' },
  { key: 'show_vram', tKey: 'metrics.vram' },
  { key: 'show_cpu', tKey: 'metrics.cpuUsage' },
  { key: 'show_cpu_temp', tKey: 'metrics.cpuTemp' },
  { key: 'show_ram', tKey: 'metrics.ram' },
];

type Tab = 'system' | 'app' | 'metrics';

const TABS: { id: Tab; tKey: string; icon: typeof InfoIcon }[] = [
  { id: 'metrics', tKey: 'settings.tabMetrics', icon: FireIcon },
  { id: 'system', tKey: 'settings.tabSystem', icon: InfoIcon },
  { id: 'app', tKey: 'settings.tabApp', icon: GearIcon },
];

export function SettingsDialog({
  onClose,
  onChanged,
  onStartTour,
}: {
  onClose: () => void;
  /** Called after a change (cache wiped / hidden restored) so the library can refresh. */
  onChanged: () => void;
  /** Re-launch the guided product tour. */
  onStartTour?: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hidden, setHidden] = useState<number | null>(null);
  const [discordId, setDiscordId] = useState('');
  const [discordSaved, setDiscordSaved] = useState(false);
  const [discordEnabled, setDiscordEnabled] = useState<boolean | null>(null);
  const [autostart, setAutostartState] = useState<boolean | null>(null);
  const [tray, setTray] = useState<boolean | null>(null);
  const [overlay, setOverlay] = useState<OverlaySettings | null>(null);
  const [shortcuts, setShortcuts] = useState<ShortcutsSettings | null>(null);
  const [sys, setSys] = useState<SystemInfo | null>(null);
  const [elevated, setElevated] = useState<boolean | null>(null);
  const [language, setLanguageState] = useState<string>('system');
  const [tab, setTab] = useState<Tab>('system');
  const { t } = useTranslation();
  // Drives the slide-in: false on first paint, flipped true after mount.
  const [shown, setShown] = useState(false);

  useEffect(() => {
    setShown(true);
    systemInfo()
      .then(setSys)
      .catch(() => setSys(null));
    isElevated()
      .then(setElevated)
      .catch(() => setElevated(null));
    hiddenCount()
      .then(setHidden)
      .catch(() => setHidden(null));
    getDiscordClientId()
      .then(setDiscordId)
      .catch(() => {});
    getAutostart()
      .then(setAutostartState)
      .catch(() => setAutostartState(null));
    getAppSettings()
      .then((s) => {
        setTray(s.minimize_to_tray);
        setOverlay(s.overlay);
        setShortcuts(s.shortcuts);
        setLanguageState(s.language ?? 'system');
        setDiscordEnabled(s.discord_enabled ?? false);
      })
      .catch(() => setTray(null));
  }, []);

  // Esc closes the drawer.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  // Optimistic local update, then send only the changed fields; Rust merges them.
  function updateOverlay(patch: Partial<OverlaySettings>) {
    setOverlay((prev) => (prev ? { ...prev, ...patch } : prev));
    patchAppSettings({ overlay: patch }).catch((e) => setError(String(e)));
  }

  function updateShortcuts(patch: Partial<ShortcutsSettings>) {
    setShortcuts((prev) => (prev ? { ...prev, ...patch } : prev));
    patchAppSettings({ shortcuts: patch }).catch((e) => setError(String(e)));
  }

  async function saveLanguage(lang: string) {
    setLanguageState(lang); // optimistic
    try {
      // patch_app_settings emits "settings-updated", which the I18nProvider listens
      // to and applies the new language across every window.
      await patchAppSettings({ language: lang });
    } catch (e) {
      setError(String(e));
    }
  }

  async function toggleAutostart() {
    if (autostart === null) return;
    const next = !autostart;
    setAutostartState(next); // optimistic
    try {
      await setAutostart(next);
    } catch (e) {
      setAutostartState(!next); // revert
      setError(String(e));
    }
  }

  async function toggleTray() {
    if (tray === null) return;
    const next = !tray;
    setTray(next); // optimistic
    try {
      await patchAppSettings({ minimize_to_tray: next });
    } catch (e) {
      setTray(!next); // revert
      setError(String(e));
    }
  }

  async function toggleDiscord() {
    if (discordEnabled === null) return;
    const next = !discordEnabled;
    setDiscordEnabled(next); // optimistic
    try {
      await patchAppSettings({ discord_enabled: next });
    } catch (e) {
      setDiscordEnabled(!next); // revert
      setError(String(e));
    }
  }

  async function saveDiscord() {
    setBusy(true);
    setError(null);
    try {
      await setDiscordClientId(discordId.trim());
      setDiscordSaved(true);
      window.setTimeout(() => setDiscordSaved(false), 2000);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function clear() {
    setBusy(true);
    setError(null);
    try {
      await clearCoverCache();
      onChanged();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function restore() {
    setBusy(true);
    setError(null);
    try {
      await restoreHidden();
      onChanged();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function restartAdmin() {
    setError(null);
    try {
      await restartAsAdmin(); // app relaunches elevated and this instance exits
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 bg-void/70 backdrop-blur-sm"
      onClick={onClose}
    >
      {/* Drawer: slides in from the left, spans 80% of the viewport width. */}
      <div
        onClick={(e) => e.stopPropagation()}
        className={`absolute inset-y-0 left-0 flex h-full w-[80vw] max-w-[1280px] border-r border-line bg-surface shadow-card transition-transform duration-300 ease-out ${
          shown ? 'translate-x-0' : '-translate-x-full'
        }`}
      >
        {/* Vertical tab rail. */}
        <nav className="flex w-64 shrink-0 flex-col border-r border-line bg-sidebar">
          <div className="flex items-center gap-2 px-5 py-5">
            <h2 className="font-display text-lg font-semibold text-ink">{t('settings.title')}</h2>
          </div>
          <div className="flex flex-1 flex-col gap-0.5 px-3">
            {TABS.map((tabDef) => {
              const active = tab === tabDef.id;
              const Icon = tabDef.icon;
              return (
                <button
                  key={tabDef.id}
                  onClick={() => setTab(tabDef.id)}
                  className={`flex items-center gap-3 border-l-2 px-3 py-2.5 text-left text-sm transition ${
                    active
                      ? 'border-accent bg-elevated font-medium text-ink'
                      : 'border-transparent text-muted hover:bg-elevated/50 hover:text-ink'
                  }`}
                >
                  <Icon className="h-4 w-4 shrink-0" />
                  <span>{t(tabDef.tKey)}</span>
                </button>
              );
            })}
          </div>
        </nav>

        {/* Close button, floating top-right of the drawer. */}
        <button
          onClick={onClose}
          className="absolute right-4 top-4 z-10 grid h-9 w-9 place-items-center text-muted transition hover:bg-elevated hover:text-ink"
          aria-label={t('common.close')}
        >
          <CloseIcon className="h-5 w-5" />
        </button>

        {/* Content for the active tab. */}
        <section className="flex-1 overflow-y-auto px-8 py-7">
          <div className="mx-auto max-w-3xl">
            {tab === 'system' && <SystemTab sys={sys} />}
            {tab === 'app' && (
              <AppTab
                busy={busy}
                hidden={hidden}
                autostart={autostart}
                tray={tray}
                shortcuts={shortcuts}
                updateShortcuts={updateShortcuts}
                discordId={discordId}
                discordSaved={discordSaved}
                discordEnabled={discordEnabled}
                setDiscordId={setDiscordId}
                saveDiscord={saveDiscord}
                toggleDiscord={toggleDiscord}
                clear={clear}
                restore={restore}
                toggleAutostart={toggleAutostart}
                toggleTray={toggleTray}
                onStartTour={onStartTour}
                language={language}
                onSetLanguage={saveLanguage}
              />
            )}
            {tab === 'metrics' && (
              <MetricsTab
                sys={sys}
                overlay={overlay}
                updateOverlay={updateOverlay}
                elevated={elevated}
                restartAdmin={restartAdmin}
                shortcuts={shortcuts}
              />
            )}

            {error && <p className="mt-6 text-sm text-accent">{error}</p>}
          </div>
        </section>
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ Tabs --- */

function MetricsTab({
  sys,
  overlay,
  updateOverlay,
  elevated,
  restartAdmin,
  shortcuts,
}: {
  sys: SystemInfo | null;
  overlay: OverlaySettings | null;
  updateOverlay: (patch: Partial<OverlaySettings>) => void;
  elevated: boolean | null;
  restartAdmin: () => void;
  shortcuts: ShortcutsSettings | null;
}) {
  const { t } = useTranslation();
  const toggleKey = formatShortcut(shortcuts?.overlay_toggle ?? DEFAULT_SHORTCUTS.overlay_toggle, t('common.keySpace')).join('+');
  // Admin is only actually needed for CPU temp (and NVIDIA FPS via PresentMon).
  const needsAdmin = !!overlay?.show_cpu_temp;
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
          {elevated === false && (
            <div className={`border p-4 ${needsAdmin ? 'border-accent/50 bg-accent/5' : 'border-line bg-elevated/30'}`}>
              <p className="mb-1 text-sm font-medium text-ink">{t('settings.mAdminTitle')}</p>
              <p className="mb-3 text-xs leading-relaxed text-muted">
                <Trans i18nKey="settings.mAdminBody" components={[<strong key="0" />, <strong key="1" />]} />
              </p>
              <Button onClick={restartAdmin} className="w-auto px-4">
                {t('settings.mRestartAdmin')}
              </Button>
            </div>
          )}
          {elevated === true && (
            <p className="text-xs text-info">{t('settings.mRunningAdmin')}</p>
          )}
          <Card
            title={t('settings.mOverlayTitle')}
            control={
              <Toggle
                on={overlay.enabled}
                onClick={() => updateOverlay({ enabled: !overlay.enabled })}
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
                        <span>
                          {t(m.tKey)}
                          {m.note && <span className="ml-1 text-[10px] text-muted">· {m.note}</span>}
                        </span>
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
                    <SegmentedControl
                      options={[
                        { label: t('settings.mSizeSmall'), value: 'xs' },
                        { label: t('settings.mSizeNormal'), value: 'sm' },
                        { label: t('settings.mSizeLarge'), value: 'base' },
                      ]}
                      value={overlay.font_size}
                      onChange={(v) => updateOverlay({ font_size: v as string })}
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

function SystemTab({ sys }: { sys: SystemInfo | null }) {
  const { t } = useTranslation();
  return (
    <div>
      <TabHeader
        title={t('settings.tabSystem')}
        desc={t('settings.sDesc')}
      />
      {!sys ? (
        <p className="text-sm text-muted">{t('settings.loading')}</p>
      ) : (
        <div className="space-y-6">
          <Card title={t('settings.sSummary')}>
            <dl className="space-y-2 text-sm">
              <InfoRow
                label={t('settings.sCpu')}
                value={`${sys.cpu} · ${t('settings.sCoresThreads', { cores: sys.cpu_cores, threads: sys.cpu_threads })}`}
              />
              <InfoRow label={t('settings.sRam')} value={fmtMem(sys.ram_total_mb)} />
              <InfoRow label={t('settings.sOs')} value={sys.os} />
              {sys.motherboard && <InfoRow label={t('settings.sMotherboard')} value={sys.motherboard} />}
            </dl>
          </Card>

          {sys.gpus.length > 0 && (
            <Card title={t('settings.sGpus')}>
              <div className="space-y-1.5">
                {sys.gpus.map((g, i) => (
                  <ListItem
                    key={g.key || `gpu-${i}`}
                    left={
                      <>
                        {g.name}
                        {g.kind && <Tag>{t(GPU_KIND_KEYS[g.kind])}</Tag>}
                        {!g.key && <Tag>{t('settings.sNoMetrics')}</Tag>}
                      </>
                    }
                    right={fmtMem(g.vram_mb)}
                  />
                ))}
              </div>
            </Card>
          )}

          {sys.displays.length > 0 && (
            <Card title={t('settings.sDisplays')}>
              <div className="space-y-1.5">
                {sys.displays.map((d, i) => (
                  <ListItem
                    key={`disp-${i}`}
                    left={
                      <>
                        {d.name}
                        {d.primary && <Tag accent>{t('settings.sPrimary')}</Tag>}
                      </>
                    }
                    right={`${d.width}×${d.height} @ ${d.refresh_hz} Hz`}
                  />
                ))}
              </div>
            </Card>
          )}

          {sys.disks.length > 0 && (
            <Card title={t('settings.sStorage')}>
              <div className="space-y-1.5">
                {sys.disks.map((d, i) => (
                  <ListItem
                    key={`disk-${i}`}
                    left={
                      <>
                        {d.name || t('settings.sDisk')}
                        {d.fs && <Tag>{d.fs}</Tag>}
                      </>
                    }
                    right={`${fmtMem(d.total_mb - d.available_mb)} / ${fmtMem(d.total_mb)}`}
                  />
                ))}
              </div>
            </Card>
          )}
        </div>
      )}
    </div>
  );
}

function AppTab({
  busy,
  hidden,
  autostart,
  tray,
  shortcuts,
  updateShortcuts,
  discordId,
  discordSaved,
  discordEnabled,
  setDiscordId,
  saveDiscord,
  toggleDiscord,
  clear,
  restore,
  toggleAutostart,
  toggleTray,
  onStartTour,
  language,
  onSetLanguage,
}: {
  busy: boolean;
  hidden: number | null;
  autostart: boolean | null;
  tray: boolean | null;
  shortcuts: ShortcutsSettings | null;
  updateShortcuts: (patch: Partial<ShortcutsSettings>) => void;
  discordId: string;
  discordSaved: boolean;
  discordEnabled: boolean | null;
  setDiscordId: (v: string) => void;
  saveDiscord: () => void;
  toggleDiscord: () => void;
  clear: () => void;
  restore: () => void;
  toggleAutostart: () => void;
  toggleTray: () => void;
  onStartTour?: () => void;
  language: string;
  onSetLanguage: (lang: string) => void;
}) {
  const { t } = useTranslation();
  const LANGS: { value: string; tKey: string }[] = [
    { value: 'system', tKey: 'settings.languageSystem' },
    { value: 'es', tKey: 'settings.languageEs' },
    { value: 'en', tKey: 'settings.languageEn' },
  ];
  return (
    <div>
      <TabHeader
        title={t('settings.tabApp')}
        desc={t('settings.appDesc')}
      />
      <div className="space-y-6">
        <Card title={t('settings.language')}>
          <p className="mb-3 text-xs leading-relaxed text-muted">{t('settings.languageDesc')}</p>
          <div className="flex overflow-hidden border border-line">
            {LANGS.map((l) => (
              <button
                key={l.value}
                onClick={() => onSetLanguage(l.value)}
                className={`flex-1 px-3 py-2 text-sm transition ${
                  language === l.value
                    ? 'bg-accent font-medium text-white'
                    : 'bg-elevated text-muted hover:text-ink'
                }`}
              >
                {t(l.tKey)}
              </button>
            ))}
          </div>
        </Card>

        {onStartTour && (
          <Card title={t('settings.tourTitle')}>
            <p className="mb-3 text-xs leading-relaxed text-muted">
              {t('settings.tourDesc')}
            </p>
            <Button onClick={onStartTour}>{t('settings.tourButton')}</Button>
          </Card>
        )}

        <Card title={t('settings.aCovers')}>
          <p className="mb-3 text-xs leading-relaxed text-muted">{t('settings.aCoversBody')}</p>
          <Button onClick={clear} disabled={busy}>
            {busy ? t('settings.aWorking') : t('settings.aClearCache')}
          </Button>
        </Card>

        <Card title={t('settings.aHidden')}>
          <p className="mb-3 text-xs leading-relaxed text-muted">
            {hidden && hidden > 0
              ? t('settings.aHiddenSome', { count: hidden })
              : t('settings.aHiddenNone')}
          </p>
          <Button onClick={restore} disabled={busy || !hidden}>
            {t('settings.aRestoreHidden')}
          </Button>
        </Card>

        {shortcuts && (
          <Card title={t('settings.aShortcuts')}>
            <p className="mb-4 text-xs leading-relaxed text-muted">{t('settings.aShortcutsBody')}</p>
            <div className="space-y-3">
              <div>
                <p className="mb-1 text-xs font-medium text-ink">{t('settings.aSpotlight')}</p>
                <p className="mb-2 text-[11px] text-muted">{t('settings.aSpotlightDesc')}</p>
                <ShortcutInput value={shortcuts.spotlight} onChange={(v) => updateShortcuts({ spotlight: v })} />
              </div>
              <div>
                <p className="mb-1 text-xs font-medium text-ink">{t('settings.aOverlayToggle')}</p>
                <p className="mb-2 text-[11px] text-muted">{t('settings.aOverlayToggleDesc')}</p>
                <ShortcutInput value={shortcuts.overlay_toggle} onChange={(v) => updateShortcuts({ overlay_toggle: v })} />
              </div>
              <div>
                <p className="mb-1 text-xs font-medium text-ink">{t('settings.aOverlaySettings')}</p>
                <p className="mb-2 text-[11px] text-muted">{t('settings.aOverlaySettingsDesc')}</p>
                <ShortcutInput value={shortcuts.overlay_settings} onChange={(v) => updateShortcuts({ overlay_settings: v })} />
              </div>
            </div>
          </Card>
        )}

        {autostart !== null && (
          <Card
            title={t('settings.aAutostart')}
            control={<Toggle on={autostart} onClick={toggleAutostart} />}
          >
            <p className="text-xs leading-relaxed text-muted">{t('settings.aAutostartBody')}</p>
          </Card>
        )}

        {tray !== null && (
          <Card
            title={t('settings.aTray')}
            control={<Toggle on={tray} onClick={toggleTray} />}
          >
            <p className="text-xs leading-relaxed text-muted">{t('settings.aTrayBody')}</p>
          </Card>
        )}

        {discordEnabled !== null && (
        <Card
          title={t('settings.aDiscord')}
          control={<Toggle on={discordEnabled} onClick={toggleDiscord} />}
        >
          <p className="text-xs leading-relaxed text-muted">{t('settings.aDiscordBody')}</p>
          {discordEnabled && (
            <div className="mt-3">
              <p className="mb-2 text-xs leading-relaxed text-muted">
                {t('settings.aDiscordIdBody')}
              </p>
              <div className="flex gap-2">
                <input
                  value={discordId}
                  onChange={(e) => setDiscordId(e.target.value)}
                  placeholder={t('settings.aDiscordIdPlaceholder')}
                  aria-label={t('settings.aDiscordIdPlaceholder')}
                  className="flex-1 border border-line bg-elevated px-3 py-2 text-sm text-ink outline-none focus:border-accent"
                />
                <Button onClick={saveDiscord} disabled={busy} className="w-auto px-4">
                  {discordSaved ? t('settings.aDiscordSaved') : t('common.save')}
                </Button>
              </div>
            </div>
          )}
        </Card>
        )}
      </div>
    </div>
  );
}



/* ----------------------------------------------------------- UI primitives --- */

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

/** Color picker: a native <input type="color"> styled to look like the Meteor design. */
function ColorPicker({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
}) {
  // While the picker is open, drag ticks only update this local draft; the
  // settings are written once, when the picker closes (native `change`) or loses
  // focus. React's `onChange` on a color input is the per-tick `input` event.
  const [draft, setDraft] = useState<string | null>(null);
  const shown = draft ?? value;
  const inputRef = useRef<HTMLInputElement>(null);
  const lastSent = useRef(value);
  const commitRef = useRef<(next: string) => void>(() => {});

  useEffect(() => {
    lastSent.current = value;
    commitRef.current = (next: string) => {
      setDraft(null);
      const commit = colorToCommit(next, lastSent.current);
      if (commit === null) return;
      lastSent.current = commit;
      onChange(commit);
    };
  }, [value, onChange]);

  useEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    const onNativeChange = () => commitRef.current(el.value);
    el.addEventListener('change', onNativeChange);
    return () => el.removeEventListener('change', onNativeChange);
  }, []);

  return (
    <div className="flex items-center justify-between gap-3">
      <span className="text-xs text-muted">{label}</span>
      <label
        className="relative h-7 w-10 cursor-pointer overflow-hidden border border-line transition hover:border-accent/60"
        title={shown}
      >
        {/* Color swatch visible surface */}
        <div className="absolute inset-0" style={{ background: shown }} />
        {/* Native color input sits on top, invisible but captures clicks */}
        <input
          ref={inputRef}
          type="color"
          value={shown}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={(e) => commitRef.current(e.currentTarget.value)}
          className="absolute inset-0 h-full w-full cursor-pointer opacity-0"
        />
      </label>
    </div>
  );
}

/** Segmented button group for selecting from a fixed set of options. */
function SegmentedControl<T extends string | number>({
  options,
  value,
  onChange,
}: {
  options: { label: string; value: T }[];
  value: T;
  onChange: (v: T) => void;
}) {
  return (
    <div className="flex overflow-hidden border border-line">
      {options.map((o) => (
        <button
          key={String(o.value)}
          onClick={() => onChange(o.value)}
          className={`flex-1 px-2 py-1.5 text-xs transition ${
            value === o.value
              ? 'bg-accent font-medium text-white'
              : 'bg-elevated text-muted hover:text-ink'
          }`}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** A button that captures a keyboard shortcut combination. */
function ShortcutInput({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const { t } = useTranslation();
  const [recording, setRecording] = useState(false);
  const [needsModifier, setNeedsModifier] = useState(false);

  useEffect(() => {
    if (!recording) return;

    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      const result = recordShortcut(e);
      if (result.kind === 'cancel') {
        setRecording(false);
        setNeedsModifier(false);
      } else if (result.kind === 'needs-modifier') {
        setNeedsModifier(true);
      } else if (result.kind === 'combo') {
        onChange(result.value);
        setRecording(false);
        setNeedsModifier(false);
      }
    };

    window.addEventListener('keydown', onKeyDown, { capture: true });
    return () => window.removeEventListener('keydown', onKeyDown, { capture: true });
  }, [recording, onChange]);

  return (
    <button
      onClick={() => setRecording(true)}
      className={`w-full border px-3 py-2 text-sm text-left transition ${
        recording ? 'border-accent bg-accent/10 text-ink' : 'border-line bg-elevated text-muted hover:text-ink'
      }`}
    >
      {recording ? t(needsModifier ? 'settings.mShortcutNeedsModifier' : 'settings.mRecording') : value}
    </button>
  );
}

function TabHeader({ title, desc }: { title: string; desc: string }) {
  return (
    <div className="mb-6">
      <h3 className="font-display text-xl font-semibold text-ink">{title}</h3>
      <p className="mt-1 text-sm text-muted">{desc}</p>
    </div>
  );
}

function Card({
  title,
  control,
  children,
}: {
  title: string;
  control?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="border border-line bg-elevated/30 p-4">
      <div className="mb-2 flex items-center justify-between gap-3">
        <p className="text-sm font-medium text-ink">{title}</p>
        {control}
      </div>
      {children}
    </div>
  );
}

function Button({
  children,
  onClick,
  disabled,
  className = '',
}: {
  children: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={`w-full border border-line bg-elevated px-4 py-2.5 text-sm font-medium text-ink transition hover:border-accent/40 disabled:opacity-50 ${className}`}
    >
      {children}
    </button>
  );
}

function Toggle({ on, onClick }: { on: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      role="switch"
      aria-checked={on}
      className={`relative h-6 w-11 shrink-0 border transition-colors ${
        on ? 'border-accent bg-accent' : 'border-line bg-elevated'
      }`}
    >
      <span
        className={`absolute top-1/2 h-4 w-4 -translate-y-1/2 transition-all ${
          on ? 'left-6 bg-white' : 'left-1 bg-muted'
        }`}
      />
    </button>
  );
}

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <dt className="shrink-0 text-muted">{label}</dt>
      <dd className="truncate text-right text-ink">{value}</dd>
    </div>
  );
}

function ListItem({ left, right }: { left: React.ReactNode; right: string }) {
  return (
    <div className="flex items-center justify-between border border-line bg-elevated px-3 py-2 text-sm">
      <span className="flex min-w-0 items-center truncate text-ink">{left}</span>
      <span className="ml-2 shrink-0 tabular-nums text-muted">{right}</span>
    </div>
  );
}

function Tag({ children, accent }: { children: React.ReactNode; accent?: boolean }) {
  return (
    <span className={`ml-1.5 text-[10px] ${accent ? 'text-accent' : 'text-muted'}`}>
      · {children}
    </span>
  );
}
