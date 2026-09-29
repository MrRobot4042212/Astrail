// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

// Opt-in to the beta update channel. Self-contained on purpose: it reads and
// patches its own setting, so the settings dialog only has to mount it. The update
// prompt re-checks on its own when the channel changes (Rust emits the event).
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { patchSettings, useAppSettings } from '@/hooks/useAppSettings';
import type { UpdateChannel } from '@/lib/types';
import { Card, Toggle } from './settings/primitives';
import { failureText } from '@/i18n/failureText';

export function UpdateChannelCard() {
  const { t } = useTranslation();
  const { settings, error: loadError } = useAppSettings();
  const [saveError, setSaveError] = useState<unknown>(null);
  const channel: UpdateChannel | null = settings ? (settings.update_channel ?? 'stable') : null;
  const failure = saveError ?? loadError;
  const error = failure ? failureText(failure) : null;

  if (channel === null && !error) return null;

  const beta = channel === 'beta';

  // Shown at once; the store puts back the saved value if the core refuses.
  const toggle = () => {
    if (channel === null) return;
    setSaveError(null);
    patchSettings({ update_channel: beta ? 'stable' : 'beta' }).catch(setSaveError);
  };

  return (
    <Card
      title={t('settings.aBeta')}
      control={channel !== null && <Toggle on={beta} onClick={toggle} label={t('settings.aBeta')} />}
    >
      <p className="text-xs leading-relaxed text-muted">{t('settings.aBetaBody')}</p>
      {beta && <p className="mt-2 text-xs leading-relaxed text-muted">{t('settings.aBetaOn')}</p>}
      {error && <p className="mt-2 break-all text-xs text-destructive">{error}</p>}
    </Card>
  );
}
