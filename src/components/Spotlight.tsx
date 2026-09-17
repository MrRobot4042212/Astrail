// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { Game } from '@/lib/types';
import { fuzzyScore } from '@/lib/fuzzy';
import { coverSrc } from '@/lib/cover';
import { SOURCE_META } from '@/lib/sources';
import { SearchIcon, PlayIcon } from './icons';

const MAX_RESULTS = 8;

function thumbOf(game: Game): string | null {
  if (game.cover_url) return coverSrc(game.cover_url) ?? null;
  return game.icon ? coverSrc(game.icon) ?? null : null;
}

/** Global launcher palette: type to fuzzy-search the library, ↑/↓ to move, Enter
 *  to launch, Esc to close. Opened from anywhere via the global hotkey. */
export function Spotlight({
  games,
  onLaunch,
  onClose,
}: {
  games: Game[];
  onLaunch: (g: Game) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [query, setQuery] = useState('');
  // Debounced like the library grid: scoring + sorting the whole library on
  // every keystroke is the one thing that makes this palette feel slow.
  const [debounced, setDebounced] = useState('');
  const [idx, setIdx] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    const t = setTimeout(() => setDebounced(query), 100);
    return () => clearTimeout(t);
  }, [query]);

  const results = useMemo(() => {
    const q = debounced.trim();
    if (!q) {
      // No query: surface favorites first, then alphabetical, as a starting set.
      return [...games]
        .sort(
          (a, b) =>
            Number(!!b.favorite) - Number(!!a.favorite) || a.name.localeCompare(b.name),
        )
        .slice(0, MAX_RESULTS);
    }
    return games
      .map((g) => ({ g, s: fuzzyScore(q, g.name) }))
      .filter((x): x is { g: Game; s: number } => x.s !== null)
      .sort((a, b) => b.s - a.s)
      .slice(0, MAX_RESULTS)
      .map((x) => x.g);
  }, [games, debounced]);

  // Keep the selection index within the current result set.
  useEffect(() => {
    setIdx((i) => Math.min(i, Math.max(0, results.length - 1)));
  }, [results.length]);

  function onKeyDown(e: React.KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      onClose();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      setIdx((i) => Math.min(i + 1, results.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setIdx((i) => Math.max(i - 1, 0));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const game = results[idx];
      if (game) {
        onLaunch(game);
        onClose();
      }
    }
  }

  return (
    <div
      className="fixed inset-0 z-[70] flex justify-center bg-void/70 p-4 pt-[12vh] backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="h-fit w-full max-w-xl overflow-hidden border border-line bg-popover shadow-card"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-3 border-b border-line px-4">
          <SearchIcon className="h-5 w-5 shrink-0 text-muted" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setIdx(0);
            }}
            onKeyDown={onKeyDown}
            placeholder={t('spotlight.placeholder')}
            className="w-full bg-transparent py-4 text-base text-ink outline-none placeholder:text-muted/60"
          />
        </div>

        {results.length === 0 ? (
          <div className="px-4 py-8 text-center text-sm text-muted">{t('spotlight.noResults')}</div>
        ) : (
          <ul className="max-h-[52vh] overflow-y-auto py-1">
            {results.map((game, i) => {
              const thumb = thumbOf(game);
              const active = i === idx;
              return (
                <li key={game.id}>
                  <button
                    onMouseEnter={() => setIdx(i)}
                    onClick={() => {
                      onLaunch(game);
                      onClose();
                    }}
                    className={`flex w-full items-center gap-3 px-3 py-2 text-left transition-colors ${
                      active ? 'bg-accent/15' : 'hover:bg-elevated'
                    }`}
                  >
                    <div className="grid h-10 w-8 shrink-0 place-items-center overflow-hidden border border-line bg-elevated">
                      {thumb ? (
                        // eslint-disable-next-line @next/next/no-img-element
                        <img src={thumb} alt="" className="h-full w-full object-cover" />
                      ) : (
                        <span className="text-sm font-bold text-accent/70">
                          {game.name.charAt(0).toUpperCase()}
                        </span>
                      )}
                    </div>
                    <span className="min-w-0 flex-1 truncate text-sm text-ink">{game.name}</span>
                    <span className="shrink-0 text-xs text-muted">
                      {SOURCE_META[game.source].label}
                    </span>
                    {active && <PlayIcon className="h-4 w-4 shrink-0 text-accent" />}
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}
