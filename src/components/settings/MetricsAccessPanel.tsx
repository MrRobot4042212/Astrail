// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useTranslation } from 'react-i18next';
import type { MetricsAccess } from '@/lib/types';
import { adminWouldHelp, cpuTempAccess, fpsAccess } from '@/lib/metricsAccess';
import { openExternal } from '@/lib/tauri';
import { Button } from './primitives';

const PAWNIO_URL = 'https://pawnio.eu/';

/** What FPS and CPU temperature need on this machine, and how to get there.
 *  `cpuTempWanted` raises the panel's emphasis when the user asked for a row
 *  that cannot be read. */
export function MetricsAccessPanel({
  access,
  cpuTempWanted,
  onRestartAdmin,
}: {
  access: MetricsAccess;
  cpuTempWanted: boolean;
  onRestartAdmin: () => void;
}) {
  const { t } = useTranslation();
  const fps = fpsAccess(access);
  const cpu = cpuTempAccess(access);
  const blocked = fps === 'blocked' || (cpuTempWanted && cpu !== 'ok');
  return (
    <div className={`border p-4 ${blocked ? 'border-accent/50 bg-accent/5' : 'border-line bg-elevated/30'}`}>
      <p className="mb-2 text-sm font-medium text-ink">{t('settings.mAccessTitle')}</p>
      <ul className="mb-3 space-y-1.5 text-xs leading-relaxed text-muted">
        <li>
          <span className="font-medium text-ink">{t('settings.mAccessFps')}</span>{' '}
          {t(`settings.mAccessFps_${fps}`)}
        </li>
        <li>
          <span className="font-medium text-ink">{t('settings.mAccessCpuTemp')}</span>{' '}
          {t(`settings.mAccessCpuTemp_${cpu}`)}
        </li>
        <li>{t('settings.mAccessRest')}</li>
      </ul>
      <div className="flex flex-wrap gap-2">
        {cpu === 'needsPawnio' || cpu === 'needsBoth' ? (
          <Button onClick={() => void openExternal(PAWNIO_URL)} className="w-auto px-4">
            {t('settings.mAccessGetPawnio')}
          </Button>
        ) : null}
        {adminWouldHelp(access) && (
          <Button onClick={onRestartAdmin} className="w-auto px-4">
            {t('settings.mRestartAdmin')}
          </Button>
        )}
      </div>
    </div>
  );
}
