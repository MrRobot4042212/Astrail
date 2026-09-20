// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { GameSource, SteamPlaytime } from '@/lib/types';
import { steamPlaytime } from '@/lib/tauri';
import { formatSpan } from '@/lib/sessionPerf';

/** The time the Steam client recorded for this entry, shown on its own line.
 *
 *  It is Steam's figure, not ours: it covers every machine of the account and is
 *  only as fresh as the client's last write, so it is never added to the time
 *  Astrail measured. Renders nothing for other stores, for an app Steam has no
 *  time for, and while the answer is pending. */
export function SteamPlaytimeLine({ id, source }: { id: string; source: GameSource }) {
  const { t, i18n } = useTranslation();
  // Keyed by the id it was asked for, so an answer for the previous entry is
  // never shown under the next one.
  const [answer, setAnswer] = useState<{ id: string; value: SteamPlaytime | null } | null>(null);

  useEffect(() => {
    if (source !== 'steam') return;
    let alive = true;
    steamPlaytime(id)
      .then((value) => {
        if (alive) setAnswer({ id, value });
      })
      .catch(() => {
        if (alive) setAnswer({ id, value: null });
      });
    return () => {
      alive = false;
    };
  }, [id, source]);

  const day = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium' }),
    [i18n.language],
  );

  const steam = source === 'steam' && answer?.id === id ? answer.value : null;
  if (!steam || steam.minutes <= 0) return null;

  return (
    <p className="mt-3 text-xs text-muted">
      <span className="text-ink/90">
        {t('detail.steamPlaytime', { time: formatSpan(steam.minutes * 60) })}
      </span>
      {steam.last_played ? (
        <> · {t('detail.steamLastPlayed', { date: day.format(steam.last_played * 1000) })}</>
      ) : null}
      {' · '}
      {t('detail.steamPlaytimeNote')}
    </p>
  );
}
