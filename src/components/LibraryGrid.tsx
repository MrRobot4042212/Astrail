'use client';

import { memo } from 'react';
import { GameCard } from './GameCard';
import type { Game } from '@/lib/types';

/**
 * The library grid.
 *
 * Extracted from `MainApp` and memoized on purpose. The map used to live inline
 * in the page body, which holds ~25 unrelated `useState`s — a toast appearing, a
 * card being dragged, a context menu opening or closing, or a single keystroke in
 * the search box all re-ran it. `GameCard` itself is memoized and every callback
 * below is referentially stable, so no card re-rendered; the cost was the parent
 * rebuilding one React element per game and running a shallow prop comparison for
 * each of them, on every one of those unrelated updates.
 *
 * Keep every prop stable (store actions or `useCallback`) or this goes back to
 * being a plain function call with extra steps.
 */
export const LibraryGrid = memo(function LibraryGrid({
  games,
  selectionMode,
  selectedIds,
  onLaunch,
  onRemove,
  onHide,
  onEditCover,
  onToggleFavorite,
  onEditCategories,
  onDragStateChange,
  onOpen,
  onContextMenu,
  onToggleSelect,
}: {
  games: Game[];
  selectionMode: boolean;
  selectedIds: Set<string>;
  onLaunch: (g: Game) => void;
  onRemove: (g: Game) => void;
  onHide: (g: Game) => void;
  onEditCover: (g: Game) => void;
  onToggleFavorite: (g: Game) => void;
  onEditCategories: (g: Game) => void;
  onDragStateChange: (active: boolean) => void;
  onOpen: (g: Game) => void;
  onContextMenu: (g: Game, x: number, y: number) => void;
  onToggleSelect: (g: Game) => void;
}) {
  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-6">
      {games.map((game, i) => (
        <GameCard
          key={game.id}
          game={game}
          // Only the entrance stagger reads it, and it caps at 24: clamping here
          // keeps the prop stable for later cards so memo holds on reorder.
          index={Math.min(i, 24)}
          onLaunch={onLaunch}
          // Manual entries are removed outright; store-owned ones are only hidden,
          // since the next scan would bring them straight back.
          onRemove={game.source === 'manual' ? onRemove : undefined}
          onHide={game.source === 'manual' ? undefined : onHide}
          onEditCover={onEditCover}
          onToggleFavorite={onToggleFavorite}
          onEditCategories={onEditCategories}
          onDragStateChange={onDragStateChange}
          onOpen={onOpen}
          onContextMenu={onContextMenu}
          selectionMode={selectionMode}
          selected={selectedIds.has(game.id)}
          onToggleSelect={onToggleSelect}
        />
      ))}
    </div>
  );
});
