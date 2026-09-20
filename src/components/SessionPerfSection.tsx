// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { Session } from '@/lib/types';
import { formatFps, formatSpan, formatTemp, measuredSessions } from '@/lib/sessionPerf';

/** What the HUD measured in the last few sessions of a game. Renders nothing
 *  until a session has been measured, so a game played with the overlay off
 *  does not get an empty table. */
export function SessionPerfSection({ history }: { history: Session[] }) {
  const { t, i18n } = useTranslation();
  const rows = useMemo(() => measuredSessions(history), [history]);
  const when = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' }),
    [i18n.language],
  );

  if (rows.length === 0) return null;

  const head = 'px-3 py-2 text-left text-[11px] font-normal uppercase tracking-wide text-muted';
  const cell = 'px-3 py-2 align-top text-ink';

  return (
    <section className="mt-10">
      <h2 className="mb-4 font-display text-sm font-semibold uppercase tracking-wide text-muted">
        {t('detail.perfTitle')}
      </h2>
      <div className="overflow-x-auto border border-line bg-surface/50">
        <table className="w-full min-w-[36rem] border-collapse text-sm">
          <thead>
            <tr className="border-b border-line">
              <th scope="col" className={head}>{t('detail.perfSession')}</th>
              <th scope="col" className={head}>{t('detail.perfLength')}</th>
              <th scope="col" className={head}>{t('detail.perfAvgFps')}</th>
              <th scope="col" className={head}>{t('detail.perfLow1')}</th>
              <th scope="col" className={head}>{t('detail.perfMaxGpu')}</th>
              <th scope="col" className={head}>{t('detail.perfMaxCpu')}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((s) => (
              <tr key={s.start} className="border-b border-line last:border-b-0">
                <td className={`${cell} whitespace-nowrap text-ink/90`}>
                  {when.format(new Date(s.start * 1000))}
                </td>
                <td className={`${cell} whitespace-nowrap text-ink/90`}>
                  {formatSpan(s.end - s.start)}
                </td>
                <td className={cell}>
                  <span className="font-display font-semibold">{formatFps(s.perf.avg_fps)}</span>
                  {s.perf.avg_fps != null && s.perf.fps_secs != null && (
                    <span className="ml-2 text-xs text-muted">
                      {t('detail.perfMeasuredOver', { time: formatSpan(s.perf.fps_secs) })}
                    </span>
                  )}
                </td>
                <td className={`${cell} font-display font-semibold`}>{formatFps(s.perf.low_1_fps)}</td>
                <td className={`${cell} font-display font-semibold`}>{formatTemp(s.perf.max_gpu_temp_c)}</td>
                <td className={`${cell} font-display font-semibold`}>{formatTemp(s.perf.max_cpu_temp_c)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="mt-2 text-xs text-muted">{t('detail.perfNote')}</p>
    </section>
  );
}
