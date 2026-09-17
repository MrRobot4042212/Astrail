// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { Game, PlayStat } from '@/lib/types';
import { username } from '@/lib/tauri';
import { coverSrc } from '@/lib/cover';
import { SOURCE_META } from '@/lib/sources';
import { PlayIcon, ClockIcon, FireIcon, GridIcon } from './icons';

/** Pretty-print a duration in seconds as "12h 30m" / "45m" / "0m". */
function fmtDuration(secs: number): string {
  const m = Math.floor(secs / 60);
  const h = Math.floor(m / 60);
  if (h > 0) return `${h}h ${m % 60}m`;
  return `${m}m`;
}


export function Home({
  games,
  playtimes,
  onOpen,
  onLaunch,
}: {
  games: Game[];
  playtimes: Record<string, PlayStat>;
  onOpen: (g: Game) => void;
  onLaunch: (g: Game) => void;
}) {
  const { t } = useTranslation();

  // The OS user's name for the greeting. Use just the first name and capitalize
  // it (handles login names like "meorb" → "Meorb"). Empty until it loads.
  const [name, setName] = useState('');
  useEffect(() => {
    username()
      .then((u) => {
        const first = u.trim().split(/\s+/)[0] ?? '';
        if (first) setName(first.charAt(0).toUpperCase() + first.slice(1));
      })
      .catch(() => {});
  }, []);

  const byId = useMemo(() => {
    const m = new Map<string, Game>();
    for (const g of games) m.set(g.id, g);
    return m;
  }, [games]);

  // Only real games count toward playtime stats; apps are tracked separately and
  // surface only in "Apps más usadas".
  const isGame = useMemo(
    () => (id: string) => {
      const g = byId.get(id);
      return !!g && g.source !== 'app';
    },
    [byId],
  );

  // --- Aggregate stats (games only) ----------------------------------------
  const stats = useMemo(() => {
    let totalSecs = 0;
    let gamesPlayed = 0;
    for (const [id, s] of Object.entries(playtimes)) {
      if (!isGame(id)) continue;
      totalSecs += s.seconds;
      if (s.seconds > 0) gamesPlayed++;
    }

    // Last 7 days, oldest → newest. Bucket boundaries are real *local midnights*
    // (not fixed 24h steps) so DST-length days bucket correctly, and each session
    // is split across every day it actually spans (a session crossing midnight is
    // counted partly in each day), so the chart reflects time-played-per-day.
    const days = Array.from({ length: 7 }, (_, i) => {
      const d = new Date();
      d.setHours(0, 0, 0, 0);
      d.setDate(d.getDate() - (6 - i));
      return { date: d, start: d.getTime(), secs: 0 };
    });
    const dayEnd = (i: number) => {
      const d = new Date(days[i].date);
      d.setDate(d.getDate() + 1);
      return d.getTime();
    };
    const weekStart = days[0].start;
    const weekEnd = dayEnd(6);

    let sessionsWeek = 0;
    for (const [id, s] of Object.entries(playtimes)) {
      if (!isGame(id)) continue;
      for (const sess of s.history ?? []) {
        const startMs = sess.start * 1000;
        const endMs = sess.end * 1000;
        if (endMs <= weekStart || startMs >= weekEnd) continue; // outside window
        sessionsWeek++;
        for (let i = 0; i < 7; i++) {
          const segStart = Math.max(startMs, days[i].start);
          const segEnd = Math.min(endMs, dayEnd(i));
          if (segEnd > segStart) days[i].secs += (segEnd - segStart) / 1000;
        }
      }
    }
    const weekSecs = days.reduce((a, d) => a + d.secs, 0);
    return { totalSecs, gamesPlayed, days, weekSecs, sessionsWeek };
  }, [playtimes, isGame]);

  // --- Recently played games (by last_played; apps excluded) ---------------
  const recent = useMemo(() => {
    return Object.entries(playtimes)
      .filter(([id, s]) => s.last_played && isGame(id))
      .sort((a, b) => (b[1].last_played ?? 0) - (a[1].last_played ?? 0))
      .slice(0, 8)
      .map(([id]) => byId.get(id)!)
      .filter(Boolean);
  }, [playtimes, byId, isGame]);

  // --- Most used, split: games vs apps (by total seconds) ------------------
  const { mostPlayedGames, mostUsedApps } = useMemo(() => {
    const ranked = Object.entries(playtimes)
      .filter(([id, s]) => s.seconds > 0 && byId.has(id))
      .sort((a, b) => b[1].seconds - a[1].seconds)
      .map(([id, s]) => ({ game: byId.get(id)!, secs: s.seconds }));
    return {
      mostPlayedGames: ranked.filter((x) => x.game.source !== 'app').slice(0, 6),
      mostUsedApps: ranked.filter((x) => x.game.source === 'app').slice(0, 6),
    };
  }, [playtimes, byId]);

  const hasData = stats.totalSecs > 0;

  // Fallback when there's no playtime yet: surface favorites first, then the
  // rest. Favorites and non-favorites are disjoint, so ids stay unique.
  const starters = useMemo(() => {
    if (hasData) return [];
    const fav = games.filter((g) => g.favorite);
    const rest = games.filter((g) => !g.favorite);
    return [...fav, ...rest].slice(0, 8);
  }, [hasData, games]);

  return (
    <div className="mx-auto max-w-6xl space-y-8">
      {/* Greeting */}
      <div>
        <h1 className="font-display text-2xl font-bold text-ink">
          {name ? t('home.welcomeBackName', { name }) : t('home.welcomeBack')}
        </h1>
        <p className="mt-1 text-sm text-muted">
          {hasData
            ? t('home.playedThisWeek', { time: fmtDuration(stats.weekSecs) })
            : t('home.noStats')}
        </p>
      </div>

      {/* Stat cards */}
      <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
        <StatCard
          icon={<ClockIcon className="h-4 w-4" />}
          label={t('home.timePlayed')}
          value={fmtDuration(stats.totalSecs)}
          hint={t('home.timePlayedHint')}
        />
        <StatCard
          icon={<FireIcon className="h-4 w-4" />}
          label={t('home.thisWeek')}
          value={fmtDuration(stats.weekSecs)}
          hint={t('home.last7')}
        />
        <StatCard
          icon={<PlayIcon className="h-4 w-4" />}
          label={t('home.sessions')}
          value={String(stats.sessionsWeek)}
          hint={t('home.last7')}
        />
        <StatCard
          icon={<GridIcon className="h-4 w-4" />}
          label={t('home.gamesPlayed')}
          value={String(stats.gamesPlayed)}
          hint={t('home.gamesWithTime')}
        />
      </div>

      {/* Continue playing */}
      {recent.length > 0 && (
        <Section title={t('home.continuePlaying')}>
          <PosterRow games={recent} playtimes={playtimes} onOpen={onOpen} onLaunch={onLaunch} />
        </Section>
      )}

      {/* Weekly activity */}
{/*       {hasData && (
        <Section title="Actividad (últimos 7 días)">
          <div className="border border-line bg-surface p-5">
            <div className="flex h-40 items-end justify-between gap-2">
              {stats.days.map((d, i) => {
                const pct = Math.round((d.secs / maxDay) * 100);
                return (
                  <div key={i} className="flex flex-1 flex-col items-center gap-2">
                    <div className="flex w-full flex-1 items-end">
                      <div
                        title={fmtDuration(d.secs)}
                        style={{ height: `${Math.max(d.secs > 0 ? 6 : 0, pct)}%` }}
                        className={`w-full transition-all ${
                          i === todayIdx ? 'bg-accent' : 'bg-accent/40'
                        }`}
                      />
                    </div>
                    <span
                      className={`text-[11px] ${
                        i === todayIdx ? 'font-semibold text-ink' : 'text-muted'
                      }`}
                    >
                      {DAY_LABELS[d.date.getDay()]}
                    </span>
                  </div>
                );
              })}
            </div>
          </div>
        </Section>
      )} */}

      {/* Most used, games and apps split into their own lists */}
      {(mostPlayedGames.length > 0 || mostUsedApps.length > 0) && (
        <div className="grid gap-6 lg:grid-cols-2">
          {mostPlayedGames.length > 0 && (
            <Section title={t('home.mostPlayedGames')}>
              <RankList items={mostPlayedGames} onOpen={onOpen} />
            </Section>
          )}
          {mostUsedApps.length > 0 && (
            <Section title={t('home.mostUsedApps')}>
              <RankList items={mostUsedApps} onOpen={onOpen} />
            </Section>
          )}
        </div>
      )}

      {/* Fresh-install fallback */}
      {!hasData && starters.length > 0 && (
        <Section title={t('home.yourLibrary')}>
          <PosterRow games={starters} playtimes={playtimes} onOpen={onOpen} onLaunch={onLaunch} />
        </Section>
      )}
    </div>
  );
}

function StatCard({
  icon,
  label,
  value,
  hint,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <div className="border border-line bg-surface p-4">
      <div className="mb-2 flex items-center gap-1.5 text-muted">
        <span className="text-accent">{icon}</span>
        <span className="text-xs font-medium uppercase tracking-wide">{label}</span>
      </div>
      <p className="font-display text-2xl font-bold text-ink">{value}</p>
      {hint && <p className="mt-0.5 text-[11px] text-muted">{hint}</p>}
    </div>
  );
}

/** Ranked vertical list (most-played games / most-used apps). */
function RankList({
  items,
  onOpen,
}: {
  items: { game: Game; secs: number }[];
  onOpen: (g: Game) => void;
}) {
  return (
    <div className="flex flex-col gap-2">
      {items.map(({ game, secs }, i) => (
        <button
          key={game.id}
          onClick={() => onOpen(game)}
          className="group flex items-center gap-3 border border-line bg-surface px-3 py-2.5 text-left transition hover:border-accent/40"
        >
          <span className="w-4 shrink-0 text-center font-display text-sm font-bold text-muted">
            {i + 1}
          </span>
          <Thumb game={game} className="h-12 w-9 shrink-0" />
          <span className="min-w-0 flex-1 truncate text-sm text-ink">{game.name}</span>
          <span className="flex shrink-0 items-center gap-1 text-xs tabular-nums text-muted">
            <ClockIcon className="h-3.5 w-3.5" />
            {fmtDuration(secs)}
          </span>
        </button>
      ))}
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section>
      <h2 className="mb-3 font-display text-base font-semibold text-ink">{title}</h2>
      {children}
    </section>
  );
}

function PosterRow({
  games,
  playtimes,
  onOpen,
  onLaunch,
}: {
  games: Game[];
  playtimes: Record<string, PlayStat>;
  onOpen: (g: Game) => void;
  onLaunch: (g: Game) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex gap-4 overflow-x-auto pb-2">
      {games.map((g) => {
        const secs = playtimes[g.id]?.seconds ?? 0;
        return (
          <div key={g.id} className="w-[150px] shrink-0">
            <div
              onClick={() => onOpen(g)}
              className="group relative aspect-[2/3] cursor-pointer overflow-hidden border border-line bg-elevated shadow-card transition hover:border-accent/40 hover:shadow-glow"
            >
              <Thumb game={g} className="h-full w-full" big />
              <div className="pointer-events-none absolute inset-0 flex items-end bg-gradient-to-t from-void via-void/30 to-transparent opacity-0 transition-opacity group-hover:opacity-100">
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    onLaunch(g);
                  }}
                  className="pointer-events-auto m-2 flex w-full items-center justify-center gap-2 bg-accent py-1.5 text-xs font-semibold text-white transition hover:bg-accent-soft"
                >
                  <PlayIcon className="h-3.5 w-3.5" />
                  {t('card.play')}
                </button>
              </div>
              <span
                className={`pointer-events-none absolute bottom-1.5 right-1.5 h-2 w-2 rounded-full ${SOURCE_META[g.source].dot}`}
              />
            </div>
            <p className="mt-1.5 truncate text-xs font-medium text-ink">{g.name}</p>
            {secs > 0 && <p className="text-[11px] text-muted">{fmtDuration(secs)}</p>}
          </div>
        );
      })}
    </div>
  );
}

/** Cover thumbnail with the app's icon / letter fallbacks (mirrors GameCard). */
function Thumb({
  game,
  className = '',
  big = false,
}: {
  game: Game;
  className?: string;
  big?: boolean;
}) {
  const cover = coverSrc(game.cover_url);
  const logo = !game.cover_url && game.icon ? coverSrc(game.icon) ?? null : null;

  if (cover) {
    // eslint-disable-next-line @next/next/no-img-element
    return <img src={cover} alt={game.name} draggable={false} loading="lazy" decoding="async" className={`object-cover ${className}`} />;
  }
  if (logo) {
    return (
      <div className={`flex items-center justify-center bg-gradient-to-b from-elevated to-surface ${className}`}>
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img src={logo} alt={game.name} draggable={false} loading="lazy" decoding="async" className="max-h-[55%] max-w-[70%] object-contain" />
      </div>
    );
  }
  return (
    <div className={`flex items-center justify-center bg-gradient-to-b from-elevated to-surface ${className}`}>
      <span className={`font-display font-bold text-accent/80 ${big ? 'text-4xl' : 'text-lg'}`}>
        {game.name.charAt(0).toUpperCase()}
      </span>
    </div>
  );
}
