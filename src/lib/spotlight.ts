// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import type { Game } from './types';
import { fuzzyScore } from './fuzzy';

/** How many entries the launcher palette lists. */
export const SPOTLIGHT_MAX = 8;

/** The palette's list for a query: the best fuzzy matches, or, with no query,
 *  favorites first and then alphabetical, as a starting set. */
export function spotlightResults(
  games: readonly Game[],
  query: string,
  max: number = SPOTLIGHT_MAX,
): Game[] {
  const q = query.trim();
  if (!q) {
    return [...games]
      .sort(
        (a, b) => Number(!!b.favorite) - Number(!!a.favorite) || a.name.localeCompare(b.name),
      )
      .slice(0, max);
  }
  return games
    .map((g) => ({ g, s: fuzzyScore(q, g.name) }))
    .filter((x): x is { g: Game; s: number } => x.s !== null)
    .sort((a, b) => b.s - a.s)
    .slice(0, max)
    .map((x) => x.g);
}

/** The game Enter launches.
 *
 *  The list on screen is computed from the query as it was 100 ms ago (the
 *  palette debounces it). Typing a name and pressing Enter at once used to
 *  launch the top of that stale list, a favorite or a match for a prefix,
 *  instead of the typed game (2026-09-27 audit, X-G2). When the typed query has
 *  moved on from the one on screen, the best match for the typed one wins; the
 *  highlighted row only counts for the list the user is looking at. */
export function spotlightPick(
  games: readonly Game[],
  shown: readonly Game[],
  shownQuery: string,
  typedQuery: string,
  index: number,
): Game | undefined {
  if (typedQuery.trim() === shownQuery.trim()) return shown[index];
  return spotlightResults(games, typedQuery, 1)[0];
}
