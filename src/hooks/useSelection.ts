// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { useCallback, useState } from 'react';

import type { Game } from '@/lib/types';

/** Multi-select over the grid. Every action keeps its identity: `toggle` is
 *  handed to memoized cards. */
export function useSelection() {
  const [active, setActive] = useState(false);
  const [ids, setIds] = useState<Set<string>>(new Set());

  /** Picks or drops one entry; the first pick is what enters selection mode. */
  const toggle = useCallback((game: Game) => {
    setActive(true);
    setIds((prev) => {
      const next = new Set(prev);
      if (next.has(game.id)) next.delete(game.id);
      else next.add(game.id);
      return next;
    });
  }, []);

  const selectAll = useCallback((games: readonly Game[]) => {
    setIds(new Set(games.map((g) => g.id)));
  }, []);

  const exit = useCallback(() => {
    setActive(false);
    setIds(new Set());
  }, []);

  return { active, ids, toggle, selectAll, exit };
}
