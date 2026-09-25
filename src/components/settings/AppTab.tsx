// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useTranslation } from 'react-i18next';
import type { SettingsModel } from '@/hooks/useSettingsModel';
import { DataBackupCard } from '../DataBackupCard';
import { DiagnosticsCard } from '../DiagnosticsCard';
import { ExternalGamesCard } from '../ExternalGamesCard';
import { UpdateChannelCard } from '../UpdateChannelCard';
import { ShortcutInput } from './ShortcutInput';
import { Button, Card, TabHeader, Toggle } from './primitives';

export function AppTab({ model, onStartTour }: { model: SettingsModel; onStartTour?: () => void }) {
  const {
    busy,
    hidden,
    autostart,
    autostartAvailable,
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
    language,
    saveLanguage: onSetLanguage,
  } = model;
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
            control={
              autostartAvailable ? (
                <Toggle on={autostart} onClick={toggleAutostart} label={t('settings.aAutostart')} />
              ) : undefined
            }
          >
            <p className="text-xs leading-relaxed text-muted">{t('settings.aAutostartBody')}</p>
            {!autostartAvailable && (
              <p className="mt-2 text-xs leading-relaxed text-muted">{t('settings.aAutostartUnavailable')}</p>
            )}
          </Card>
        )}

        {tray !== null && (
          <Card
            title={t('settings.aTray')}
            control={<Toggle on={tray} onClick={toggleTray} label={t('settings.aTray')} />}
          >
            <p className="text-xs leading-relaxed text-muted">{t('settings.aTrayBody')}</p>
          </Card>
        )}

        {discordEnabled !== null && (
        <Card
          title={t('settings.aDiscord')}
          control={<Toggle on={discordEnabled} onClick={toggleDiscord} label={t('settings.aDiscord')} />}
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

        <ExternalGamesCard />
        <UpdateChannelCard />
        <DataBackupCard />
        <DiagnosticsCard />
      </div>
    </div>
  );
}
