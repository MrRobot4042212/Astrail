// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { useEffect, useMemo, useState } from 'react';

import {
  filterIsAlive,
  libraryCounts,
  sidebarCategories,
  visibleGames,
} from '@/lib/libraryView';
import type { Category, Game, PlayStat } from '@/lib/types';
import type { Filter } from '@/components/Sidebar';
import type { SortKey } from '@/components/TopBar';

const QUERY_DEBOUNCE_MS = 150;

/** Filter, search and sort of the library screen, and what they derive: the
 *  sidebar's categories and counts, and the entries the grid shows. The rules
 *  themselves live in `lib/libraryView`. */
export function useLibraryView(
  games: Game[],
  categoryMeta: Category[],
  playtimes: Record<string, PlayStat>,
) {
  const [filter, setFilter] = useState<Filter>('home');
  const [query, setQuery] = useState('');
  // The input always shows the raw query, but the grid re-filters only once
  // typing pauses, so a large library is not fuzzy-matched per keystroke.
  const [debouncedQuery, setDebouncedQuery] = useState('');
  const [sort, setSort] = useState<SortKey>('name');

  useEffect(() => {
    const timer = setTimeout(() => setDebouncedQuery(query), QUERY_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [query]);

  const categories = useMemo(() => sidebarCategories(categoryMeta, games), [categoryMeta, games]);
  const categoryNames = useMemo(() => categories.map((c) => c.name), [categories]);
  const counts = useMemo(() => libraryCounts(games, categoryNames), [games, categoryNames]);
  const visible = useMemo(
    () => visibleGames(games, filter, debouncedQuery, sort, playtimes),
    [games, filter, debouncedQuery, sort, playtimes],
  );

  // If the active filter disappears from the sidebar (its last game left the
  // category), fall back to everything instead of an empty, unreachable view.
  useEffect(() => {
    if (!filterIsAlive(filter, categoryNames)) setFilter('all');
  }, [filter, categoryNames]);

  // The dashboard replaces the grid on the home filter, unless searching.
  const showingHome = filter === 'home' && !query.trim();

  return {
    filter,
    setFilter,
    query,
    setQuery,
    sort,
    setSort,
    categories,
    categoryNames,
    counts,
    visible,
    showingHome,
  };
}
