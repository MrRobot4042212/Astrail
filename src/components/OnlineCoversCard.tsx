// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

// Whether covers are looked up on the internet. The lookup sends each game's name
// to IGDB: the one thing Astrail sends about the library on its own, so it has a
// switch, here and in the first-run setup. Self-contained like the cards next to
// it; the core is what stops the lookup when the value changes.
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { patchSettings, useAppSettings } from '@/hooks/useAppSettings';
import { privacyUrl } from '@/lib/privacy';
import { openExternal } from '@/lib/tauri';
import { Card, Toggle } from './settings/primitives';
import { failureText } from '@/i18n/failureText';

export function OnlineCoversCard() {
  const { t, i18n } = useTranslation();
  const { settings, error: loadError } = useAppSettings();
  const [saveError, setSaveError] = useState<unknown>(null);
  const on = settings ? (settings.online_covers ?? true) : null;
  const failure = saveError ?? loadError;
  const error = failure ? failureText(failure) : null;

  if (on === null && !error) return null;

  // Shown at once; the store puts back the saved value if the core refuses.
  const toggle = () => {
    if (on === null) return;
    setSaveError(null);
    patchSettings({ online_covers: !on }).catch(setSaveError);
  };

  return (
    <Card
      title={t('settings.aOnlineCovers')}
      control={on !== null && <Toggle on={on} onClick={toggle} label={t('settings.aOnlineCovers')} />}
    >
      <p className="text-xs leading-relaxed text-muted">{t('settings.aOnlineCoversBody')}</p>
      <button
        type="button"
        onClick={() => openExternal(privacyUrl(i18n.language)).catch(setSaveError)}
        className="mt-2 text-xs text-accent underline hover:text-ink"
      >
        {t('settings.aPrivacyPolicy')}
      </button>
      {error && <p className="mt-2 break-all text-xs text-destructive">{error}</p>}
    </Card>
  );
}
