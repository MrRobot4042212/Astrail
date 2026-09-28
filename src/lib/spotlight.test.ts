// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { describe, expect, it } from 'vitest';

import type { Game } from './types';
import { spotlightPick, spotlightResults } from './spotlight';

const game = (id: string, name: string, extra: Partial<Game> = {}): Game => ({
  id,
  name,
  source: 'steam',
  app_id: null,
  executable: null,
  install_dir: null,
  cover_url: null,
  launch_uri: null,
  favorite: false,
  categories: [],
  ...extra,
});

const library = [
  game('a', 'Age of Empires II', { favorite: true }),
  game('h', 'Hearthstone'),
  game('p', 'Portal'),
  game('p2', 'Portal 2'),
  game('t', 'TEKKEN 8'),
];

describe('spotlightResults', () => {
  it('starts with favorites, then alphabetical', () => {
    expect(spotlightResults(library, '').map((g) => g.id)).toEqual(['a', 'h', 'p', 'p2', 't']);
  });

  it('ranks the best match first and caps the list', () => {
    expect(spotlightResults(library, 'portal 2')[0].id).toBe('p2');
    expect(spotlightResults(library, '', 2)).toHaveLength(2);
    expect(spotlightResults(library, 'zzzz')).toEqual([]);
  });
});

describe('spotlightPick', () => {
  it('launches the highlighted row of the list on screen', () => {
    const shown = spotlightResults(library, 'portal');
    expect(spotlightPick(library, shown, 'portal', 'portal', 1)?.id).toBe(shown[1].id);
  });

  it('launches the typed game when Enter beats the debounce', () => {
    // Regression (X-G2): "tekken" typed and Enter pressed within 100 ms launched
    // the first favorite of the empty-query list on screen.
    const shown = spotlightResults(library, '');
    expect(spotlightPick(library, shown, '', 'tekken', 0)?.id).toBe('t');
    // Same with a stale prefix on screen: "port" still showed Portal first.
    const prefix = spotlightResults(library, 'port');
    expect(spotlightPick(library, prefix, 'port', 'portal 2', 0)?.id).toBe('p2');
  });

  it('launches nothing when the typed query matches nothing', () => {
    const shown = spotlightResults(library, 'port');
    expect(spotlightPick(library, shown, 'port', 'portzzz', 0)).toBeUndefined();
  });
});
