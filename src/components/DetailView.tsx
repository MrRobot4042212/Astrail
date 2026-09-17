// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import type { Game, PlayStat, Session } from '@/lib/types';
import {
  getPlaytime,
  gameDirSize,
  openGameFolder,
  openExternal,
  resolveCoverHires,
  userScreenshots,
} from '@/lib/tauri';
import { coverSrc } from '@/lib/cover';
import { SOURCE_META } from '@/lib/sources';
import {
  ArrowLeftIcon,
  PlayIcon,
  StarIcon,
  TagIcon,
  ImageIcon,
  FolderIcon,
  EyeOffIcon,
  TrashIcon,
  ClockIcon,
  CloseIcon,
  AppIcon,
  GridIcon,
  YoutubeIcon,
  TwitchIcon,
  RedditIcon,
  GlobeIcon,
  GamepadIcon,
  ZapIcon,
  GearIcon,
} from './icons';

/** Screenshots rendered before the "show all" button. Decoding a full gallery
 *  at once is the most expensive thing this page can do. */
const SHOTS_INITIAL = 12;

type T = (key: string, opts?: Record<string, unknown>) => string;

/** A community link. Opens in the user's browser through a Rust command with a
 *  host allowlist — a plain `<a href>` would navigate this window away from the
 *  app, and there is no back button in a launcher. */
function ExternalLink({
  href,
  className,
  children,
}: {
  href: string;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={() => {
        openExternal(href).catch(() => {});
      }}
      className={className}
    >
      {children}
    </button>
  );
}

function formatPlaytime(seconds: number, t: T): string {
  if (seconds < 60) return t('detail.noRecord');
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (h === 0) return `${m} min`;
  return m === 0 ? `${h} h` : `${h} h ${m} min`;
}

/** Seconds played within the last `days` days (from session history). */
function recentSeconds(history: Session[], days: number): number {
  const cutoff = Date.now() / 1000 - days * 86400;
  return history.reduce((acc, s) => acc + (s.end >= cutoff ? s.end - s.start : 0), 0);
}

function formatSize(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${Math.round(bytes / 1024 ** 2)} MB`;
  return `${Math.round(bytes / 1024)} KB`;
}

function formatLastPlayed(ts: number, t: T): string {
  const diff = Date.now() / 1000 - ts;
  const day = 86400;
  if (diff < day) return t('detail.today');
  if (diff < 2 * day) return t('detail.yesterday');
  if (diff < 30 * day) return t('detail.daysAgo', { count: Math.floor(diff / day) });
  return new Date(ts * 1000).toLocaleDateString();
}

export function DetailView({
  game,
  onBack,
  onLaunch,
  onToggleFavorite,
  onToggleType,
  onEditCover,
  onEditCategories,
  onRemove,
  onHide,
}: {
  game: Game;
  onBack: () => void;
  onLaunch: (g: Game) => void;
  onToggleFavorite: (g: Game) => void;
  onToggleType?: (g: Game) => void;
  onEditCover: (g: Game) => void;
  onEditCategories: (g: Game) => void;
  onRemove?: (g: Game) => void;
  onHide?: (g: Game) => void;
}) {
  const { t } = useTranslation();
  const [play, setPlay] = useState<PlayStat>({ seconds: 0, history: [] });
  const [size, setSize] = useState<number | null | undefined>(undefined);
  const [shots, setShots] = useState<string[]>([]);
  const [shot, setShot] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<'galeria' | 'grial' | 'local'>('galeria');

  // Applications aren't games: skip the screenshots + tabs section.
  const isApp = game.source === 'app';

  useEffect(() => {
    let alive = true;
    setShots([]);
    setShotLimit(SHOTS_INITIAL);
    getPlaytime(game.id)
      .then((p) => alive && setPlay(p))
      .catch(() => {});
    if (!isApp) {
      // The user's own screenshots (Steam / Game Bar), not promotional art.
      userScreenshots(game.id)
        .then((s) => alive && setShots(s))
        .catch(() => {});
    }
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [game.id, game.name, isApp]);

  // Hi-res cover for the hero. The grid stores `t_cover_big` (264×374) since
  // that is what a 240px card needs; this pulls the 2x variant for the one place
  // it is rendered large, reusing the cached IGDB image id (no new search).
  const [shotLimit, setShotLimit] = useState(SHOTS_INITIAL);
  const [hero, setHero] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    setHero(null);
    if (!isApp) {
      resolveCoverHires(game.name)
        .then((u) => alive && setHero(u))
        .catch(() => {});
    }
    return () => {
      alive = false;
    };
  }, [game.name, isApp]);

  useEffect(() => {
    let alive = true;
    setSize(undefined);
    if (game.install_dir) {
      gameDirSize(game.id)
        .then((s) => alive && setSize(s ?? null))
        .catch(() => alive && setSize(null));
    } else {
      setSize(null);
    }
    return () => {
      alive = false;
    };
  }, [game.id, game.install_dir]);

  // Refresh playtime live when a session this game ends.
  useEffect(() => {
    const un = listen<string>('playtime-updated', (e) => {
      if (e.payload === game.id) getPlaytime(game.id).then(setPlay).catch(() => {});
    });
    return () => {
      un.then((f) => f());
    };
  }, [game.id]);

  const logo = game.cover_url ? null : game.icon ? coverSrc(game.icon) ?? null : null;
  // Prefer the hi-res variant once it arrives; the grid's smaller file is what
  // `cover_url` points at, and this is the one place it is rendered large.
  const cover = coverSrc(hero ?? game.cover_url);
  // A user screenshot makes a richer backdrop than the cover; else fall back to it.
  const backdrop = (shots[0] ? coverSrc(shots[0]) : undefined) ?? cover;
  const meta = SOURCE_META[game.source];
  const SourceIcon = meta.Icon;

  return (
    <div data-tour="detail" className="relative h-full overflow-y-auto bg-background">
      {/* Backdrop */}
      <div className="pointer-events-none absolute inset-x-0 top-0 h-[420px] overflow-hidden">
        {backdrop && (
          // eslint-disable-next-line @next/next/no-img-element
          <img
            src={backdrop}
            alt=""
            className="h-full w-full scale-110 object-cover opacity-25 blur-2xl"
          />
        )}
        <div className="absolute inset-0 bg-gradient-to-b from-background/40 via-background/70 to-background" />
      </div>

      {/* Back bar */}
      <div className="sticky top-0 z-20 flex items-center gap-3 bg-background/80 px-6 py-3 backdrop-blur">
        <button
          onClick={onBack}
          className="flex items-center gap-2 px-2 py-1.5 text-sm text-muted transition hover:text-ink"
        >
          <ArrowLeftIcon className="h-[18px] w-[18px]" />
          {t('sidebar.library')}
        </button>
      </div>

      <div className="relative mx-auto max-w-5xl px-6 pb-16 pt-16">
        {/* Hero */}
        <div className="flex flex-col gap-6 sm:flex-row">
          <div className="aspect-[2/3] w-44 shrink-0 self-center overflow-hidden border border-line bg-elevated shadow-card sm:self-start">
            {logo ? (
              <div className="grid h-full w-full place-items-center bg-gradient-to-b from-elevated to-surface p-7">
                {/* eslint-disable-next-line @next/next/no-img-element */}
                <img src={logo} alt={game.name} className="max-h-[60%] max-w-[80%] object-contain drop-shadow-lg" />
              </div>
            ) : cover ? (
              // eslint-disable-next-line @next/next/no-img-element
              <img src={cover} alt={game.name} className="h-full w-full object-cover" />
            ) : (
              <div className="grid h-full w-full place-items-center text-5xl font-bold text-accent/70">
                {game.name.charAt(0).toUpperCase()}
              </div>
            )}
          </div>

          <div className="min-w-0 flex-1">
            <div className="mb-2 flex items-center gap-2 text-xs uppercase tracking-wide text-muted">
              <SourceIcon className="h-4 w-4" />
              {meta.label}
            </div>
            <h1 className="font-display text-3xl font-bold leading-tight text-ink">
              {game.name}
            </h1>

            {/* Actions */}
            <div className="mt-5 flex flex-wrap items-center gap-2">
              <button
                onClick={() => onLaunch(game)}
                className="flex items-center gap-2 bg-accent px-6 py-2.5 text-sm font-semibold text-white transition hover:bg-accent-soft"
              >
                <PlayIcon className="h-4 w-4" />
                {t('common.play')}
              </button>
              <IconBtn
                title={game.favorite ? t('detail.favoriteRemove') : t('detail.favoriteAdd')}
                active={game.favorite}
                onClick={() => onToggleFavorite(game)}
              >
                <StarIcon className="h-[18px] w-[18px]" fill={game.favorite ? 'currentColor' : 'none'} />
              </IconBtn>
              <IconBtn title={t('card.categories')} onClick={() => onEditCategories(game)}>
                <TagIcon className="h-[18px] w-[18px]" />
              </IconBtn>
              <IconBtn title={t('card.changeCover')} onClick={() => onEditCover(game)}>
                <ImageIcon className="h-[18px] w-[18px]" />
              </IconBtn>
              {onToggleType && (
                <IconBtn
                  title={game.source === 'app' ? t('menu.markAsGame') : t('menu.markAsApp')}
                  onClick={() => onToggleType(game)}
                >
                  {game.source === 'app' ? (
                    <GridIcon className="h-[18px] w-[18px]" />
                  ) : (
                    <AppIcon className="h-[18px] w-[18px]" />
                  )}
                </IconBtn>
              )}
              {game.install_dir && (
                <IconBtn
                  title={t('menu.openFolder')}
                  onClick={() => openGameFolder(game.id)}
                >
                  <FolderIcon className="h-[18px] w-[18px]" />
                </IconBtn>
              )}
              {onHide && (
                <IconBtn title={t('common.hide')} onClick={() => onHide(game)}>
                  <EyeOffIcon className="h-[18px] w-[18px]" />
                </IconBtn>
              )}
              {onRemove && (
                <IconBtn title={t('common.remove')} onClick={() => onRemove(game)}>
                  <TrashIcon className="h-[18px] w-[18px]" />
                </IconBtn>
              )}
            </div>

            {/* Categories assigned */}
            {game.categories && game.categories.length > 0 && (
              <div className="mt-4 flex flex-wrap gap-2">
                {game.categories.map((c) => (
                  <span key={c} className="bg-elevated px-2 py-0.5 text-xs text-ink">
                    {c}
                  </span>
                ))}
              </div>
            )}
          </div>
        </div>

        {/* Activity metrics */}
        <div className="mt-8 grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-5">
          <Metric label={t('home.timePlayed')} value={formatPlaytime(play.seconds, t)} icon={<ClockIcon className="h-4 w-4" />} />
          <Metric
            label={t('detail.last14')}
            value={formatPlaytime(recentSeconds(play.history, 14), t)}
          />
          <Metric label={t('home.sessions')} value={play.history.length > 0 ? String(play.history.length) : '—'} />
          <Metric
            label={t('detail.lastPlayed')}
            value={play.last_played ? formatLastPlayed(play.last_played, t) : t('common.never')}
          />
          <Metric
            label={t('detail.size')}
            value={size === undefined ? '…' : size === null ? '—' : formatSize(size)}
          />
        </div>

        {!isApp && (
          <>
            {/* Tabs navigation */}
            <div className="mt-10 mb-6 flex overflow-x-auto border-b border-line">
              {[
                { id: 'galeria', label: t('detail.tabGallery') },
                { id: 'grial', label: t('detail.tabGrail') },
                { id: 'local', label: t('detail.tabLocal') },
              ].map((tab) => (
                <button
                  key={tab.id}
                  onClick={() => setActiveTab(tab.id as 'galeria' | 'grial' | 'local')}
                  className={`whitespace-nowrap px-6 py-3 font-display text-sm font-semibold uppercase tracking-wide transition ${
                    activeTab === tab.id
                      ? 'border-b-2 border-accent text-accent'
                      : 'text-muted hover:text-ink'
                  }`}
                >
                  {tab.label}
                </button>
              ))}
            </div>

            {/* GALERÍA: the user's own captures (Steam / Game Bar). */}
            {activeTab === 'galeria' && (
              <div className="space-y-10">
                <Section title={t('detail.myScreenshots')}>
                  {shots.length > 0 ? (
                    <div className="grid grid-cols-2 gap-3 lg:grid-cols-3">
                      {shots.slice(0, shotLimit).map((s) => (
                        <button
                          key={s}
                          onClick={() => setShot(s)}
                          className="group overflow-hidden border border-line bg-elevated"
                        >
                          {/* eslint-disable-next-line @next/next/no-img-element */}
                          <img
                            src={coverSrc(s)}
                            alt=""
                            loading="lazy"
                            decoding="async"
                            className="aspect-video w-full object-cover transition group-hover:scale-105"
                          />
                        </button>
                      ))}
                    </div>
                  ) : (
                    <p className="text-sm text-muted">{t('detail.noScreenshots')}</p>
                  )}
                  {shots.length > shotLimit && (
                    <button
                      onClick={() => setShotLimit(shots.length)}
                      className="mt-4 border border-line px-4 py-2 text-sm text-ink transition hover:border-accent/50 hover:bg-elevated"
                    >
                      {t('detail.showAllScreenshots', { count: shots.length })}
                    </button>
                  )}
                </Section>
              </div>
            )}

            {/* SANTO GRIAL: external search links (no API, built from the game name). */}
            {activeTab === 'grial' && (
              <div className="space-y-10">
                <Section title={t('detail.communities')}>
                  <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-3">
                    <ExternalLink href={`https://www.pcgamingwiki.com/w/index.php?search=${encodeURIComponent(game.name)}`} className="flex items-center gap-3 border border-line bg-elevated px-4 py-3 text-sm text-ink transition hover:border-accent/50 hover:bg-elevated/80">
                      <div className="text-accent"><GamepadIcon className="h-5 w-5" /></div>
                      <div className="min-w-0 flex-1 truncate font-medium">PCGamingWiki</div>
                    </ExternalLink>
                    <ExternalLink href={`https://www.nexusmods.com/search/?gsearch=${encodeURIComponent(game.name)}`} className="flex items-center gap-3 border border-line bg-elevated px-4 py-3 text-sm text-ink transition hover:border-accent/50 hover:bg-elevated/80">
                      <div className="text-accent"><GearIcon className="h-5 w-5" /></div>
                      <div className="min-w-0 flex-1 truncate font-medium">Nexus Mods</div>
                    </ExternalLink>
                    <ExternalLink href={`https://www.protondb.com/search?q=${encodeURIComponent(game.name)}`} className="flex items-center gap-3 border border-line bg-elevated px-4 py-3 text-sm text-ink transition hover:border-accent/50 hover:bg-elevated/80">
                      <div className="text-accent"><AppIcon className="h-5 w-5" /></div>
                      <div className="min-w-0 flex-1 truncate font-medium">ProtonDB</div>
                    </ExternalLink>
                    <ExternalLink href={`https://duckduckgo.com/?q=!ducky+${encodeURIComponent(game.name + ' wiki fandom')}`} className="flex items-center gap-3 border border-line bg-elevated px-4 py-3 text-sm text-ink transition hover:border-accent/50 hover:bg-elevated/80">
                      <div className="text-accent"><GlobeIcon className="h-5 w-5" /></div>
                      <div className="min-w-0 flex-1 truncate font-medium">Wiki / Fandom</div>
                    </ExternalLink>
                    <ExternalLink href={`https://www.youtube.com/results?search_query=${encodeURIComponent(game.name + ' gameplay')}`} className="flex items-center gap-3 border border-line bg-elevated px-4 py-3 text-sm text-ink transition hover:border-accent/50 hover:bg-elevated/80">
                      <div className="text-accent"><YoutubeIcon className="h-5 w-5" /></div>
                      <div className="min-w-0 flex-1 truncate font-medium">YouTube (Gameplay)</div>
                    </ExternalLink>
                    <ExternalLink href={`https://www.twitch.tv/directory/search?term=${encodeURIComponent(game.name)}`} className="flex items-center gap-3 border border-line bg-elevated px-4 py-3 text-sm text-ink transition hover:border-accent/50 hover:bg-elevated/80">
                      <div className="text-accent"><TwitchIcon className="h-5 w-5" /></div>
                      <div className="min-w-0 flex-1 truncate font-medium">Twitch</div>
                    </ExternalLink>
                    <ExternalLink href={`https://duckduckgo.com/?q=!ducky+${encodeURIComponent(game.name + ' subreddit')}`} className="flex items-center gap-3 border border-line bg-elevated px-4 py-3 text-sm text-ink transition hover:border-accent/50 hover:bg-elevated/80">
                      <div className="text-accent"><RedditIcon className="h-5 w-5" /></div>
                      <div className="min-w-0 flex-1 truncate font-medium">Reddit</div>
                    </ExternalLink>
                    <ExternalLink href={`https://www.speedrun.com/search?q=${encodeURIComponent(game.name)}`} className="flex items-center gap-3 border border-line bg-elevated px-4 py-3 text-sm text-ink transition hover:border-accent/50 hover:bg-elevated/80">
                      <div className="text-accent"><ZapIcon className="h-5 w-5" /></div>
                      <div className="min-w-0 flex-1 truncate font-medium">Speedrun.com</div>
                    </ExternalLink>
                    <ExternalLink href={`https://howlongtobeat.com/?q=${encodeURIComponent(game.name)}`} className="flex items-center gap-3 border border-line bg-elevated px-4 py-3 text-sm text-ink transition hover:border-accent/50 hover:bg-elevated/80">
                      <div className="text-accent"><ClockIcon className="h-5 w-5" /></div>
                      <div className="min-w-0 flex-1 truncate font-medium">HowLongToBeat</div>
                    </ExternalLink>
                  </div>
                </Section>
              </div>
            )}

            {/* LOCAL */}
            {activeTab === 'local' && (
              <div className="space-y-10">
                <Section title={t('detail.diskInfo')}>
                  <dl className="max-w-3xl divide-y divide-line text-sm border border-line p-4 bg-surface/30">
                    <Row label={t('detail.installDir')} value={game.install_dir ?? t('detail.unknown')} />
                    {game.executable && <Row label={t('detail.executable')} value={game.executable} />}
                    <Row
                      label={t('detail.diskSize')}
                      value={size === undefined ? t('detail.calculating') : size === null ? '—' : formatSize(size)}
                    />
                    <Row label={t('detail.sourcePlatform')} value={SOURCE_META[game.source]?.label ?? game.source} />
                  </dl>
                  {game.install_dir && (
                    <button
                      onClick={() => openGameFolder(game.id)}
                      className="mt-4 flex items-center gap-2 border border-line px-4 py-2 text-sm text-ink transition hover:border-accent/50 hover:bg-elevated"
                    >
                      <FolderIcon className="h-4 w-4 text-accent" />
                      {t('detail.openContainingFolder')}
                    </button>
                  )}
                </Section>
              </div>
            )}
          </>
        )}
      </div>

      {/* Screenshot lightbox */}
      {shot && (
        <div
          className="fixed inset-0 z-50 grid place-items-center bg-void/90 p-8"
          onClick={() => setShot(null)}
        >
          <button className="absolute right-6 top-6 text-muted hover:text-ink">
            <CloseIcon className="h-6 w-6" />
          </button>
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img src={coverSrc(shot)} alt="" className="max-h-full max-w-full border border-line" />
        </div>
      )}
    </div>
  );
}

function IconBtn({
  children,
  title,
  active,
  onClick,
}: {
  children: React.ReactNode;
  title: string;
  active?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      title={title}
      onClick={onClick}
      className={`grid h-10 w-10 place-items-center border transition ${
        active
          ? 'border-accent text-accent'
          : 'border-line text-muted hover:border-accent/50 hover:text-ink'
      }`}
    >
      {children}
    </button>
  );
}

function Metric({
  label,
  value,
  icon,
}: {
  label: string;
  value: string;
  icon?: React.ReactNode;
}) {
  return (
    <div className="border border-line bg-surface/50 p-4">
      <div className="flex items-center gap-1.5 text-[11px] uppercase tracking-wide text-muted">
        {icon}
        {label}
      </div>
      <div className="mt-1 truncate font-display text-lg font-semibold text-ink" title={value}>
        {value}
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="mt-10">
      <h2 className="mb-4 font-display text-sm font-semibold uppercase tracking-wide text-muted">
        {title}
      </h2>
      {children}
    </section>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex gap-4 py-2">
      <dt className="w-48 shrink-0 text-muted">{label}</dt>
      <dd className="min-w-0 flex-1 break-all text-ink/90">{value}</dd>
    </div>
  );
}
