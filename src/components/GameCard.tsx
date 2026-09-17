// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { memo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { Game } from '@/lib/types';
import { SOURCE_META } from '@/lib/sources';
import { coverSrc } from '@/lib/cover';
import { PlayIcon, TrashIcon, ImageIcon, EyeOffIcon, StarIcon, TagIcon } from './icons';

function fallbackChain(game: Game): string[] {
  // Covers come from IGDB (cached on disk) or a manual override; if it's missing
  // we show the letter placeholder rather than another source.
  const src = coverSrc(game.cover_url);
  return src ? [src] : [];
}

/** How far the card tilts toward the cursor, in degrees. */
const MAX_TILT = 11;

export const GameCard = memo(function GameCard({
  game,
  onLaunch,
  onRemove,
  onEditCover,
  onHide,
  onToggleFavorite,
  onEditCategories,
  onDragStateChange,
  onOpen,
  onContextMenu,
  selectionMode = false,
  selected = false,
  onToggleSelect,
  index = 0,
}: {
  game: Game;
  onLaunch: (g: Game) => void;
  onRemove?: (g: Game) => void;
  onEditCover?: (g: Game) => void;
  onHide?: (g: Game) => void;
  onToggleFavorite?: (g: Game) => void;
  onEditCategories?: (g: Game) => void;
  /** Notifies the page when this card starts/stops being dragged, so the sidebar
   *  can invite valid drop targets. */
  onDragStateChange?: (active: boolean) => void;
  /** Open the game's detail page (clicking the cover, not a tool button). */
  onOpen?: (g: Game) => void;
  /** Right-click → context menu at the cursor. */
  onContextMenu?: (g: Game, x: number, y: number) => void;
  /** Multi-select mode: clicking toggles selection instead of opening detail. */
  selectionMode?: boolean;
  selected?: boolean;
  onToggleSelect?: (g: Game) => void;
  /** Position in the grid, for the staggered entrance animation delay. */
  index?: number;
}) {
  const { t } = useTranslation();
  // For apps without a cover, show the real icon extracted from the exe (centered
  // on a tile). A manual cover override still wins over it.
  const logo = game.cover_url ? null : game.icon ? coverSrc(game.icon) ?? null : null;
  const chain = fallbackChain(game);
  const [stage, setStage] = useState(0);
  const src = chain[stage];
  const exhausted = stage >= chain.length;

  // Cursor-follow 3D tilt + glare applied directly to the DOM — no React
  // re-render per mousemove. `ref` targets the perspective container;
  // `glareRef` targets the glare overlay so we update its gradient too.
  const ref = useRef<HTMLDivElement>(null);
  const glareRef = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState(false);

  function handleMove(e: React.MouseEvent) {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const px = (e.clientX - r.left) / r.width;
    const py = (e.clientY - r.top) / r.height;
    const rx = (0.5 - py) * MAX_TILT;
    const ry = (px - 0.5) * MAX_TILT;
    el.style.transform = `perspective(700px) rotateX(${rx}deg) rotateY(${ry}deg) scale(1.05)`;
    el.style.transition = 'transform 90ms ease-out';
    if (glareRef.current) {
      glareRef.current.style.background = `radial-gradient(circle at ${px * 100}% ${py * 100}%, rgba(255,255,255,0.22), transparent 50%)`;
    }
  }

  function handleLeave() {
    const el = ref.current;
    if (!el) return;
    el.style.transform = 'perspective(700px) rotateX(0deg) rotateY(0deg) scale(1)';
    el.style.transition = 'transform 300ms ease-out';
  }

  function handleDragStart(e: React.DragEvent) {
    // Carry the game id so sidebar drop targets (Favoritos / categories) know
    // what was dropped. Flatten the tilt first for a clean drag image.
    e.dataTransfer.setData('text/plain', game.id);
    e.dataTransfer.effectAllowed = 'copy';
    handleLeave();

    // Apply drag-shrink scale imperatively (bypasses React render cycle).
    if (ref.current) {
      ref.current.style.transform = 'scale(0.96)';
    }

    // Custom drag image: a small theme-aware chip (cover + name) instead of the
    // whole oversized card. Rendered offscreen just long enough to snapshot it.
    const cs = getComputedStyle(document.documentElement);
    const color = (n: string) => `rgb(${cs.getPropertyValue(n).trim()})`;
    const ghost = document.createElement('div');
    ghost.style.cssText = `position:fixed;top:-9999px;left:-9999px;display:flex;align-items:center;gap:10px;padding:8px 12px;max-width:240px;background:${color('--sidebar')};color:${color('--foreground')};border:1px solid ${color('--border')};box-shadow:0 12px 30px rgba(0,0,0,.55);font:600 13px/1.2 Oxanium,system-ui,sans-serif;`;
    if (src) {
      const img = document.createElement('img');
      img.src = src;
      img.style.cssText = 'width:28px;height:40px;object-fit:cover;flex:none;';
      ghost.appendChild(img);
    }
    const label = document.createElement('span');
    label.textContent = game.name;
    label.style.cssText = 'white-space:nowrap;overflow:hidden;text-overflow:ellipsis;';
    ghost.appendChild(label);
    document.body.appendChild(ghost);
    e.dataTransfer.setDragImage(ghost, 18, 20);
    setTimeout(() => ghost.remove(), 0);

    setDragging(true);
    onDragStateChange?.(true);
  }

  // Mount-on-hover for the tool chrome (see the render below).
  const [hovered, setHovered] = useState(false);

  function handleDragEnd() {
    // Reset transform to identity on drag end.
    if (ref.current) {
      ref.current.style.transform = '';
      ref.current.style.transition = 'transform 300ms ease-out';
    }
    setDragging(false);
    onDragStateChange?.(false);
  }

  return (
    <div
      className="animate-card-in"
      style={{
        animationDelay: `${Math.min(index, 24) * 30}ms`,
        // Skip layout/paint for cards outside the viewport; the intrinsic size
        // keeps the scrollbar honest (cards are a 2/3 box at ~240px wide).
        contentVisibility: 'auto',
        containIntrinsicSize: '360px',
      }}
    >
    <div
      ref={ref}
      data-tour="game-card"
      draggable={!selectionMode}
      onDragStart={handleDragStart}
      onDragEnd={handleDragEnd}
      onMouseMove={handleMove}
      onMouseLeave={handleLeave}
      onClick={(e) => {
        // In selection mode (or with Ctrl/Cmd) clicking toggles selection;
        // otherwise it opens the detail page.
        if (selectionMode || e.ctrlKey || e.metaKey) {
          e.preventDefault();
          onToggleSelect?.(game);
        } else {
          onOpen?.(game);
        }
      }}
      onContextMenu={(e) => {
        if (!onContextMenu) return;
        e.preventDefault();
        onContextMenu(game, e.clientX, e.clientY);
      }}
      onMouseEnter={() => setHovered(true)}
      className={`group relative aspect-[2/3] overflow-hidden border bg-elevated shadow-card hover:shadow-glow ${
        selectionMode ? 'cursor-pointer' : 'cursor-grab active:cursor-grabbing'
      } ${
        selected
          ? 'border-accent ring-2 ring-inset ring-accent'
          : dragging
            ? 'border-accent opacity-40 grayscale'
            : 'border-line hover:border-accent/40'
      }`}
    >
      {/* Selection checkbox (only in selection mode) */}
      {selectionMode && (
        <div
          className={`pointer-events-none absolute left-2 top-2 z-20 grid h-6 w-6 place-items-center border-2 transition ${
            selected ? 'border-accent bg-accent text-white' : 'border-white/70 bg-void/50'
          }`}
        >
          {selected && (
            <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="3">
              <path d="M5 13l4 4L19 7" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          )}
        </div>
      )}

      {logo ? (
        <div className="flex h-full w-full items-center justify-center bg-gradient-to-b from-elevated to-surface p-8">
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img
            src={logo}
            alt={game.name}
            loading="lazy"
            decoding="async"
            draggable={false}
            className="max-h-[55%] max-w-[70%] object-contain drop-shadow-lg"
          />
        </div>
      ) : !exhausted && src ? (
        // eslint-disable-next-line @next/next/no-img-element
        <img
          src={src}
          alt={game.name}
          loading="lazy"
          decoding="async"
          draggable={false}
          onError={() => setStage((s) => s + 1)}
          className="h-full w-full object-cover"
        />
      ) : (
        <div className="flex h-full w-full flex-col items-center justify-center gap-2 bg-gradient-to-b from-elevated to-surface p-3 text-center">
          <span className="font-display text-4xl font-bold text-accent/80">
            {game.name.charAt(0).toUpperCase()}
          </span>
          <span className="line-clamp-3 text-xs text-muted">{game.name}</span>
        </div>
      )}

      {/* Glare: a soft highlight tracking the cursor for a glossy, modern sheen.
          The background gradient is updated imperatively via glareRef. */}
      <div
        ref={glareRef}
        aria-hidden
        className="pointer-events-none absolute inset-0 opacity-0 transition-opacity duration-200 group-hover:opacity-100"
        style={{ background: 'radial-gradient(circle at 50% 50%, rgba(255,255,255,0.22), transparent 50%)' }}
      />

      {/* Hover overlay (hidden in selection mode) */}
      <div
        className={`pointer-events-none absolute inset-0 flex flex-col justify-end bg-gradient-to-t from-void via-void/40 to-transparent opacity-0 transition-opacity duration-200 ${
          selectionMode ? '' : 'group-hover:opacity-100'
        }`}
      >
        <div className="pointer-events-auto p-3">
          <p className="mb-2 line-clamp-2 text-sm font-medium text-ink">
            {game.name}
          </p>
          <button
            onClick={(e) => {
              e.stopPropagation();
              onLaunch(game);
            }}
            className="flex w-full items-center justify-center gap-2 rounded-lg bg-accent py-2 text-sm font-semibold text-white transition-colors hover:bg-accent-soft"
          >
            <PlayIcon className="h-4 w-4" />
            {t('card.play')}
          </button>
        </div>
      </div>

      {/* Favorite star: persistent (gold) when favorited, else shown on hover.
          Mounted only when favorited or once hovered — each of these carries a
          backdrop-filter, i.e. its own compositor layer. */}
      {onToggleFavorite && !selectionMode && (game.favorite || hovered) && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onToggleFavorite(game);
          }}
          title={game.favorite ? t('card.removeFavorite') : t('card.addFavorite')}
          className={`pointer-events-auto absolute left-2 top-2 grid h-8 w-8 place-items-center rounded-lg bg-void/70 backdrop-blur transition ${
            game.favorite
              ? 'text-ink opacity-100'
              : 'text-muted opacity-0 hover:text-ink group-hover:opacity-100'
          }`}
        >
          <StarIcon className="h-4 w-4" fill={game.favorite ? 'currentColor' : 'none'} />
        </button>
      )}

      {/* Top-right tools (hidden in selection mode, mounted on first hover) */}
      {hovered && !selectionMode && (
      <div
        className={`pointer-events-none absolute right-2 top-2 flex gap-1.5 opacity-0 transition ${
          selectionMode ? 'hidden' : 'group-hover:opacity-100'
        }`}
      >
        {onEditCategories && (
          <button
            onClick={(e) => {
              e.stopPropagation();
              onEditCategories(game);
            }}
            title={t('card.categories')}
            className="pointer-events-auto grid h-8 w-8 place-items-center rounded-lg bg-void/70 text-muted backdrop-blur transition hover:text-accent"
          >
            <TagIcon className="h-4 w-4" />
          </button>
        )}
        {onEditCover && (
          <button
            onClick={(e) => {
              e.stopPropagation();
              onEditCover(game);
            }}
            title={t('card.changeCover')}
            className="pointer-events-auto grid h-8 w-8 place-items-center rounded-lg bg-void/70 text-muted backdrop-blur transition hover:text-accent"
          >
            <ImageIcon className="h-4 w-4" />
          </button>
        )}
        {onHide && (
          <button
            onClick={(e) => {
              e.stopPropagation();
              onHide(game);
            }}
            title={t('card.hideFromLibrary')}
            className="pointer-events-auto grid h-8 w-8 place-items-center rounded-lg bg-void/70 text-muted backdrop-blur transition hover:text-accent"
          >
            <EyeOffIcon className="h-4 w-4" />
          </button>
        )}
        {onRemove && (
          <button
            onClick={(e) => {
              e.stopPropagation();
              onRemove(game);
            }}
            title={t('card.removeFromLibrary')}
            className="pointer-events-auto grid h-8 w-8 place-items-center rounded-lg bg-void/70 text-muted backdrop-blur transition hover:text-accent"
          >
            <TrashIcon className="h-4 w-4" />
          </button>
        )}
      </div>
      )}

      {/* Source dot */}
      <span
        className={`pointer-events-none absolute bottom-2 right-2 z-10 h-2 w-2 rounded-full ${SOURCE_META[game.source].dot}`}
        title={SOURCE_META[game.source].label}
      />

      {/* Animated glow border on hover (spins only while hovered). Hidden in
          selection mode, where the accent selection ring takes over. */}
      {!selectionMode && hovered && <div className="card-beam" aria-hidden />}
    </div>
    </div>
  );
});
