// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

// Opt-in to the beta update channel. Self-contained on purpose: it reads and
// patches its own setting, so the settings dialog only has to mount it. The update
// prompt re-checks on its own when the channel changes (Rust emits the event).
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getAppSettings, patchAppSettings } from '@/lib/tauri';
import type { UpdateChannel } from '@/lib/types';
import { Card, Toggle } from './settings/primitives';
import { failureText } from '@/i18n/failureText';

export function UpdateChannelCard() {
  const { t } = useTranslation();
  const [channel, setChannel] = useState<UpdateChannel | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    getAppSettings()
      .then((s) => {
        if (!cancelled) setChannel(s.update_channel ?? 'stable');
      })
      .catch((e) => {
        if (!cancelled) setError(failureText(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (channel === null && !error) return null;

  const beta = channel === 'beta';

  const toggle = () => {
    if (channel === null) return;
    const next: UpdateChannel = beta ? 'stable' : 'beta';
    setChannel(next); // optimistic
    setError(null);
    patchAppSettings({ update_channel: next }).catch((e) => {
      setChannel(channel); // revert
      setError(failureText(e));
    });
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
