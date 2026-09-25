// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

// What the library screen shows, derived from the list of entries: the sidebar's
// categories and counts, the filtered and ordered grid, and the category merge
// the bulk actions apply. No React, no IPC, so every rule here has a test.

import type { Category, Game, GameSource, PlayStat } from './types';
import { SOURCE_ORDER } from './sources';
import { fuzzyScore } from './fuzzy';

/** What the sidebar selected: a fixed view, a store, or a user category. */
export type Filter = 'home' | 'all' | 'favorites' | GameSource | `cat:${string}`;

/** Grid sort order chosen in the top bar. */
export type SortKey = 'name' | 'recent' | 'played';

/** The sidebar's selection. Structural twin of `Filter` in `Sidebar.tsx`, kept
 *  as a string here so this module does not import from a component. */
export type ViewFilter = string;

export type ViewSort = 'name' | 'recent' | 'played';

/** Folder to reveal for a game: its install dir, else the exe's parent. */
export function folderOf(game: Pick<Game, 'install_dir' | 'executable'>): string | null {
  if (game.install_dir) return game.install_dir;
  const exe = game.executable;
  if (!exe) return null;
  const i = Math.max(exe.lastIndexOf('\\'), exe.lastIndexOf('/'));
  return i > 0 ? exe.slice(0, i) : null;
}

/**
 * Categories shown in the sidebar: explicitly created ones first, in their saved
 * order (they carry an icon and persist while empty), then any that exist only
 * because an entry uses them, alphabetically. Names compare case-insensitively
 * and the explicit entry wins, so its icon and position are kept.
 */
/** Category names are one category whatever their case, as in the backend
 *  (`storage.rs` compares them with `eq_ignore_ascii_case`). */
export function sameCategory(a: string, b: string): boolean {
  return a.toLowerCase() === b.toLowerCase();
}

export function sidebarCategories(meta: readonly Category[], games: readonly Game[]): Category[] {
  const result: Category[] = [];
  const seen = new Set<string>();
  for (const c of meta) {
    const key = c.name.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(c);
  }
  const inUse = new Set<string>();
  for (const g of games) for (const c of g.categories ?? []) inUse.add(c);
  // One row per category whatever its spelling: counts and filters match names
  // regardless of case, so every spelling is reachable from that row.
  for (const name of [...inUse].sort((a, b) => a.localeCompare(b))) {
    const key = name.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    result.push({ name, icon: null });
  }
  return result;
}

/** Sidebar counters in one pass over the library: `all`, `favorites`, one per
 *  source and one `cat:<name>` per listed category. */
export function libraryCounts(games: readonly Game[], categoryNames: readonly string[]): Record<string, number> {
  const base: Record<string, number> = { all: 0, favorites: 0 };
  for (const source of SOURCE_ORDER) base[source] = 0;
  // Listed spelling by lower-cased name, so "rpg" counts under the "RPG" row.
  const listed = new Map<string, string>();
  for (const name of categoryNames) {
    base[`cat:${name}`] = 0;
    listed.set(name.toLowerCase(), `cat:${name}`);
  }
  for (const g of games) {
    base.all++;
    if (g.favorite) base.favorites++;
    base[g.source] = (base[g.source] ?? 0) + 1;
    const counted = new Set<string>();
    for (const c of g.categories ?? []) {
      const key = listed.get(c.toLowerCase());
      if (key && !counted.has(key)) {
        counted.add(key);
        base[key]++;
      }
    }
  }
  return base;
}

function inFilter(g: Game, filter: ViewFilter): boolean {
  // Home has no grid of its own; a query typed there searches everything.
  if (filter === 'all' || filter === 'home') return true;
  if (filter === 'favorites') return !!g.favorite;
  if (filter.startsWith('cat:')) {
    const name = filter.slice(4);
    return g.categories?.some((c) => sameCategory(c, name)) ?? false;
  }
  return g.source === filter;
}

/**
 * The grid's entries. A query takes over the ordering (fuzzy score, best
 * first); without one the chosen sort applies, ties broken by name.
 */
export function visibleGames(
  games: readonly Game[],
  filter: ViewFilter,
  query: string,
  sort: ViewSort,
  playtimes: Readonly<Record<string, PlayStat>>,
): Game[] {
  const matching = games.filter((g) => inFilter(g, filter));

  const q = query.trim();
  if (q) {
    return matching
      .map((g) => ({ g, s: fuzzyScore(q, g.name) }))
      .filter((x): x is { g: Game; s: number } => x.s !== null)
      .sort((a, b) => b.s - a.s)
      .map((x) => x.g);
  }

  const secs = (id: string) => playtimes[id]?.seconds ?? 0;
  const last = (id: string) => playtimes[id]?.last_played ?? 0;
  const byName = (a: Game, b: Game) => a.name.localeCompare(b.name);
  if (sort === 'played') return matching.sort((a, b) => secs(b.id) - secs(a.id) || byName(a, b));
  if (sort === 'recent') return matching.sort((a, b) => last(b.id) - last(a.id) || byName(a, b));
  return matching.sort(byName);
}

/** `current` plus every name in `added` it does not already have, compared
 *  case-insensitively. Returns `current` itself when nothing is new. */
export function addCategories(current: readonly string[] | undefined, added: readonly string[]): string[] {
  const merged = [...(current ?? [])];
  for (const c of added) {
    if (!merged.some((x) => x.toLowerCase() === c.toLowerCase())) merged.push(c);
  }
  return merged.length === (current ?? []).length ? ((current ?? []) as string[]) : merged;
}

/**
 * What a bulk "add to categories" has to write: `id -> merged list`, for the
 * selected entries that actually gain a name. One plan feeds both the optimistic
 * update and the writes, so the screen and the store cannot disagree, and an
 * entry that already has every category costs no write.
 */
export function planCategoryAdd(
  games: readonly Game[],
  ids: ReadonlySet<string>,
  added: readonly string[],
): Map<string, string[]> {
  const plan = new Map<string, string[]>();
  for (const g of games) {
    if (!ids.has(g.id)) continue;
    const merged = addCategories(g.categories, added);
    // `addCategories` only ever appends, so a longer list is a changed one.
    if (merged.length !== (g.categories?.length ?? 0)) plan.set(g.id, merged);
  }
  return plan;
}

/** Does the filter still point at something the sidebar lists? A category filter
 *  dies with its category (renamed, deleted, or its last entry left). */
export function filterIsAlive(filter: ViewFilter, categoryNames: readonly string[]): boolean {
  if (!filter.startsWith('cat:')) return true;
  const name = filter.slice(4);
  return categoryNames.some((c) => sameCategory(c, name));
}
