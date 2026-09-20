// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

// Everything the library screen can do to an entry or a category. Each action
// paints its result first and writes second; a write that fails re-reads the
// library, which is what puts the screen back (`mergeScan` inherits nothing
// from the list on screen).

import { useCallback, type Dispatch, type ReactNode, type SetStateAction } from 'react';
import { useTranslation } from 'react-i18next';

import {
  hideGame,
  launchGame,
  openGameFolder,
  removeCategory,
  removeGame,
  setCategories,
  setCategoryOrder,
  setFavorite,
  setGameType,
} from '@/lib/tauri';
import { planCategoryAdd } from '@/lib/libraryView';
import type { Category, Game } from '@/lib/types';
import type { Filter } from '@/components/Sidebar';

export interface ConfirmRequest {
  title: string;
  message: ReactNode;
  confirmLabel: string;
  onConfirm: () => void;
}

interface Deps {
  games: Game[];
  setGames: Dispatch<SetStateAction<Game[]>>;
  refresh: (showSplash?: boolean) => Promise<void>;
  refreshCategories: () => Promise<void>;
  flash: (msg: string) => void;
  ask: (req: ConfirmRequest) => void;
  selectedIds: ReadonlySet<string>;
  exitSelection: () => void;
  filter: Filter;
  setFilter: (f: Filter) => void;
  setSelectedId: Dispatch<SetStateAction<string | null>>;
}

export function useLibraryActions({
  games,
  setGames,
  refresh,
  refreshCategories,
  flash,
  ask,
  selectedIds,
  exitSelection,
  filter,
  setFilter,
  setSelectedId,
}: Deps) {
  const { t } = useTranslation();

  // The handlers wrapped in `useCallback` are handed to memoized `GameCard`s and
  // must keep their identity: functional setters only, no render state read
  // from the closure.

  const launch = useCallback(
    async (game: Game) => {
      try {
        await launchGame(game.id);
        flash(t('toast.launching', { name: game.name }));
      } catch (e) {
        flash(t('toast.launchFailed', { error: String(e) }));
      }
    },
    [t, flash],
  );

  const openFolder = useCallback(
    (game: Game) => {
      openGameFolder(game.id).catch(() => flash(t('toast.folderOpenFailed')));
    },
    [t, flash],
  );

  const toggleFavorite = useCallback(
    async (game: Game) => {
      const next = !game.favorite;
      setGames((prev) => prev.map((g) => (g.id === game.id ? { ...g, favorite: next } : g)));
      try {
        await setFavorite(game.id, next);
      } catch {
        refresh();
      }
    },
    [refresh, setGames],
  );

  // Reclassify an entry between game and application. The backend re-derives the
  // real source on each scan, so we optimistically mirror its mapping (→app sets
  // 'app'; →game from an app becomes the generic 'windows' game source) and then
  // refresh so covers/grouping reconcile (a now-game gets IGDB art).
  const toggleType = useCallback(
    async (game: Game) => {
      const toApp = game.source !== 'app';
      const optimisticSource = toApp ? 'app' : 'windows';
      setGames((prev) =>
        prev.map((g) => (g.id === game.id ? { ...g, source: optimisticSource, icon: undefined } : g)),
      );
      flash(toApp ? t('toast.toApp', { name: game.name }) : t('toast.toGame', { name: game.name }));
      try {
        await setGameType(game.id, toApp ? 'app' : 'game');
      } catch {
        /* refresh below reconciles on failure */
      }
      refresh();
    },
    [t, flash, refresh, setGames],
  );

  /** Takes the entry off the screen (and out of the detail page), then writes. */
  const drop = useCallback(
    async (game: Game, write: (id: string) => Promise<unknown>) => {
      setSelectedId((cur) => (cur === game.id ? null : cur));
      setGames((prev) => prev.filter((g) => g.id !== game.id));
      try {
        await write(game.id);
      } catch {
        refresh();
      }
    },
    [refresh, setGames, setSelectedId],
  );

  const remove = useCallback(
    (game: Game) => {
      ask({
        title: t('confirm.removeTitle'),
        message: t('confirm.removeBody', { name: game.name }),
        confirmLabel: t('common.remove'),
        onConfirm: () => drop(game, removeGame),
      });
    },
    [t, ask, drop],
  );

  const hide = useCallback(
    (game: Game) => {
      ask({
        title: t('confirm.hideTitle'),
        message: t('confirm.hideBody', { name: game.name }),
        confirmLabel: t('common.hide'),
        onConfirm: () => drop(game, hideGame),
      });
    },
    [t, ask, drop],
  );

  /** A game card was dragged onto Favorites or a category in the sidebar. */
  async function dropOnSidebar(target: Filter, gameId: string) {
    const game = games.find((g) => g.id === gameId);
    if (!game) return;

    if (target === 'favorites') {
      if (game.favorite) return;
      setGames((prev) => prev.map((g) => (g.id === gameId ? { ...g, favorite: true } : g)));
      flash(t('toast.toFavorites', { name: game.name }));
      try {
        await setFavorite(gameId, true);
      } catch {
        refresh();
      }
      return;
    }

    if (target.startsWith('cat:')) {
      const name = target.slice(4);
      const plan = planCategoryAdd([game], new Set([gameId]), [name]);
      const next = plan.get(gameId);
      if (!next) return; // already there
      setGames((prev) => prev.map((g) => (g.id === gameId ? { ...g, categories: next } : g)));
      flash(t('toast.toCategory', { name: game.name, category: name }));
      try {
        await setCategories(gameId, next);
      } catch {
        refresh();
      }
    }
  }

  // --- Bulk actions over the selection ---------------------------------------

  /** Waits for a batch already painted optimistically. If any write failed, the
   *  library is re-read and the user is told how many did not stick, instead of
   *  a success message over a screen that lies until the next scan. */
  async function settled(writes: Promise<unknown>[]): Promise<boolean> {
    const failed = (await Promise.allSettled(writes)).filter((r) => r.status === 'rejected').length;
    if (failed === 0) return true;
    refresh();
    flash(t('toast.bulkSaveFailed', { count: failed }));
    return false;
  }

  async function bulkFavorite(value: boolean) {
    const ids = [...selectedIds];
    setGames((prev) => prev.map((g) => (selectedIds.has(g.id) ? { ...g, favorite: value } : g)));
    if (!(await settled(ids.map((id) => setFavorite(id, value))))) return;
    flash(t(value ? 'toast.favoritedCount' : 'toast.unfavoritedCount', { count: ids.length }));
  }

  async function bulkAddCategories(cats: string[]) {
    const count = selectedIds.size;
    const plan = planCategoryAdd(games, selectedIds, cats);
    setGames((prev) =>
      prev.map((g) => {
        const merged = plan.get(g.id);
        return merged ? { ...g, categories: merged } : g;
      }),
    );
    if (!(await settled([...plan].map(([id, merged]) => setCategories(id, merged))))) return;
    flash(t('toast.categoriesAddedTo', { count }));
  }

  async function doBulkHide() {
    const ids = [...selectedIds];
    setGames((prev) => prev.filter((g) => !selectedIds.has(g.id)));
    exitSelection();
    if (!(await settled(ids.map((id) => hideGame(id))))) return;
    flash(t('toast.hiddenCount', { count: ids.length }));
  }

  function bulkHide() {
    ask({
      title: t('confirm.hideSelectionTitle'),
      message: t('confirm.hideSelectionBody', { count: selectedIds.size }),
      confirmLabel: t('common.hide'),
      onConfirm: doBulkHide,
    });
  }

  // --- Categories --------------------------------------------------------------

  async function reorderCategories(names: string[]) {
    try {
      await setCategoryOrder(names);
      await refreshCategories();
    } catch {
      flash(t('toast.reorderCategoriesFailed'));
    }
  }

  async function doDeleteCategory(cat: Category) {
    try {
      await removeCategory(cat.name);
      if (filter === `cat:${cat.name}`) setFilter('all');
      await refreshCategories();
      refresh();
      flash(t('toast.categoryDeleted', { name: cat.name }));
    } catch {
      flash(t('toast.categoryDeleteFailed'));
    }
  }

  function deleteCategory(cat: Category) {
    ask({
      title: t('confirm.deleteCategoryTitle'),
      message: t('confirm.deleteCategoryBody', { name: cat.name }),
      confirmLabel: t('common.delete'),
      onConfirm: () => doDeleteCategory(cat),
    });
  }

  return {
    launch,
    openFolder,
    toggleFavorite,
    toggleType,
    remove,
    hide,
    dropOnSidebar,
    bulkFavorite,
    bulkAddCategories,
    bulkHide,
    reorderCategories,
    deleteCategory,
  };
}
