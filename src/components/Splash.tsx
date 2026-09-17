// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { MeteorIcon } from './icons';

/** A few meteorites with varied lane, size, delay and speed for a shower effect. */
const METEORS = [
  { left: '6%', size: 'h-5 w-5', delay: '0s', dur: '2.1s' },
  { left: '24%', size: 'h-8 w-8', delay: '0.7s', dur: '2.7s' },
  { left: '42%', size: 'h-4 w-4', delay: '1.2s', dur: '1.9s' },
  { left: '58%', size: 'h-7 w-7', delay: '0.3s', dur: '2.4s' },
  { left: '76%', size: 'h-5 w-5', delay: '1.5s', dur: '2.2s' },
  { left: '86%', size: 'h-6 w-6', delay: '0.9s', dur: '2.9s' },
];

/** First-run loading screen. Shows while the library loads and the initial cover
 *  pass runs, with real progress (done/total) so it never feels stuck. A "Entrar"
 *  escape hatch appears after a few seconds in case cover resolution is slow. */
export function Splash({
  progress,
  onSkip,
  exiting = false,
}: {
  progress: { done: number; total: number };
  onSkip: () => void;
  /** When true, fade out (the parent keeps us mounted until the fade finishes). */
  exiting?: boolean;
}) {
  const { t } = useTranslation();
  const [showSkip, setShowSkip] = useState(false);

  useEffect(() => {
    const id = window.setTimeout(() => setShowSkip(true), 4000);
    return () => window.clearTimeout(id);
  }, []);

  const indeterminate = progress.total === 0;
  const pct = indeterminate ? 0 : Math.round((progress.done / progress.total) * 100);
  const phase = indeterminate
    ? t('splash.preparing')
    : t('splash.loadingCovers', { done: progress.done, total: progress.total });

  return (
    <div
      className={`fixed inset-0 z-[100] flex flex-col items-center justify-center gap-8 bg-background no-select animate-fade-in transition-opacity duration-500 ${
        exiting ? 'opacity-0' : 'opacity-100'
      }`}
    >
      {/* Animated mark: a shower of falling meteorites */}
      <div className="relative h-28 w-56 overflow-hidden">
        {METEORS.map((m, i) => (
          <MeteorIcon
            key={i}
            aria-hidden
            className={`animate-meteor-fall absolute top-0 text-accent ${m.size}`}
            style={{ left: m.left, animationDelay: m.delay, animationDuration: m.dur }}
          />
        ))}
      </div>

      <div className="text-center">
        <h1 className="font-display text-3xl font-bold tracking-[0.2em] text-ink">
          METEOR
        </h1>
        <p className="mt-1 font-display text-xs uppercase tracking-[0.3em] text-muted">
          {t('app.tagline')}
        </p>
      </div>

      {/* Progress */}
      <div className="w-64">
        <div className="h-1 w-full overflow-hidden bg-elevated">
          {indeterminate ? (
            <div className="animate-meteor-slide h-full w-1/3 bg-accent" />
          ) : (
            <div
              className="h-full bg-accent transition-[width] duration-300 ease-out"
              style={{ width: `${pct}%` }}
            />
          )}
        </div>
        <p className="mt-3 text-center text-xs tabular-nums text-muted">{phase}</p>
      </div>

      <button
        onClick={onSkip}
        className={`text-xs uppercase tracking-widest text-muted transition-opacity hover:text-ink ${
          showSkip ? 'opacity-100' : 'pointer-events-none opacity-0'
        }`}
      >
        {t('splash.enterNow')}
      </button>
    </div>
  );
}
