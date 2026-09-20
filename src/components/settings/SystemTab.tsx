// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useTranslation } from 'react-i18next';
import type { SystemInfo } from '@/lib/types';
import { fmtMem, GPU_KIND_KEYS } from '@/lib/systemFormat';
import { Card, InfoRow, ListItem, TabHeader, Tag } from './primitives';

export function SystemTab({ sys }: { sys: SystemInfo | null }) {
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
