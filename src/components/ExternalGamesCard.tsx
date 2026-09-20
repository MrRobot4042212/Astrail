// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

// Whether games started outside Astrail (from their store, a shortcut…) are timed
// and get the HUD. Self-contained on purpose: it reads and patches its own setting,
// so the settings dialog only has to mount it. Rust starts or stops the foreground
// hook when the value changes.
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getAppSettings, patchAppSettings } from '@/lib/tauri';
import { Card, Toggle } from './settings/primitives';

export function ExternalGamesCard() {
  const { t } = useTranslation();
  const [on, setOn] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    getAppSettings()
      .then((s) => {
        if (!cancelled) setOn(s.track_external_games ?? true);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (on === null && !error) return null;

  const toggle = () => {
    if (on === null) return;
    const next = !on;
    setOn(next); // optimistic
    setError(null);
    patchAppSettings({ track_external_games: next }).catch((e) => {
      setOn(on); // revert
      setError(String(e));
    });
  };

  return (
    <Card
      title={t('settings.aExternal')}
      control={on !== null && <Toggle on={on} onClick={toggle} label={t('settings.aExternal')} />}
    >
      <p className="text-xs leading-relaxed text-muted">{t('settings.aExternalBody')}</p>
      <p className="mt-2 text-xs leading-relaxed text-muted">{t('settings.aExternalLimit')}</p>
      {error && <p className="mt-2 break-all text-xs text-destructive">{error}</p>}
    </Card>
  );
}
