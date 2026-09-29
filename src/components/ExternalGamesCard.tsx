// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

// Whether games started outside Astrail (from their store, a shortcut…) are timed
// and get the HUD. Self-contained on purpose: it reads and patches its own setting,
// so the settings dialog only has to mount it. Rust starts or stops the foreground
// hook when the value changes.
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { patchSettings, useAppSettings } from '@/hooks/useAppSettings';
import { Card, Toggle } from './settings/primitives';
import { failureText } from '@/i18n/failureText';

export function ExternalGamesCard() {
  const { t } = useTranslation();
  const { settings, error: loadError } = useAppSettings();
  const [saveError, setSaveError] = useState<unknown>(null);
  const on = settings ? (settings.track_external_games ?? true) : null;
  const failure = saveError ?? loadError;
  const error = failure ? failureText(failure) : null;

  if (on === null && !error) return null;

  // Shown at once; the store puts back the saved value if the core refuses.
  const toggle = () => {
    if (on === null) return;
    setSaveError(null);
    patchSettings({ track_external_games: !on }).catch(setSaveError);
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
