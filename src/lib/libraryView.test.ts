// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { describe, expect, it } from 'vitest';

import type { Game, PlayStat } from './types';
import {
  addCategories,
  filterIsAlive,
  folderOf,
  libraryCounts,
  planCategoryAdd,
  sidebarCategories,
  visibleGames,
} from './libraryView';

const game = (id: string, extra: Partial<Game> = {}): Game => ({
  id,
  name: `Game ${id}`,
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

const stat = (seconds: number, last_played: number | null = null): PlayStat => ({
  seconds,
  last_played,
  history: [],
});

describe('folderOf', () => {
  it('prefers the install dir', () => {
    expect(folderOf({ install_dir: 'D:\\Games\\X', executable: 'D:\\Games\\X\\bin\\x.exe' })).toBe('D:\\Games\\X');
  });

  it('falls back to the folder holding the executable, either separator', () => {
    expect(folderOf({ executable: 'C:\\Apps\\Tool\\tool.exe', install_dir: null })).toBe('C:\\Apps\\Tool');
    expect(folderOf({ executable: 'C:/Apps/Tool/tool.exe', install_dir: null })).toBe('C:/Apps/Tool');
  });

  it('has nothing to reveal for a bare file name or no path at all', () => {
    expect(folderOf({ executable: 'tool.exe', install_dir: null })).toBeNull();
    expect(folderOf({ executable: null, install_dir: null })).toBeNull();
  });
});

describe('sidebarCategories', () => {
  it('lists created categories first, in their saved order, then the in-use ones by name', () => {
    const meta = [
      { name: 'Zeta', icon: 'star' },
      { name: 'Alpha', icon: null },
    ];
    const games = [game('1', { categories: ['Roguelike', 'Zeta'] }), game('2', { categories: ['Coop'] })];
    expect(sidebarCategories(meta, games).map((c) => c.name)).toEqual(['Zeta', 'Alpha', 'Coop', 'Roguelike']);
  });

  it('keeps the created entry, icon included, over an in-use spelling of it', () => {
    const meta = [{ name: 'RPG', icon: 'sword' }];
    const games = [game('1', { categories: ['rpg'] })];
    expect(sidebarCategories(meta, games)).toEqual([{ name: 'RPG', icon: 'sword' }]);
  });

  it('drops a created category repeated with another case', () => {
    const meta = [
      { name: 'Indie', icon: 'a' },
      { name: 'indie', icon: 'b' },
    ];
    expect(sidebarCategories(meta, [])).toEqual([{ name: 'Indie', icon: 'a' }]);
  });

  it('keeps an empty created category', () => {
    expect(sidebarCategories([{ name: 'Backlog', icon: null }], [game('1')])).toHaveLength(1);
  });
});

describe('libraryCounts', () => {
  const games = [
    game('1', { favorite: true, categories: ['Coop'] }),
    game('2', { source: 'epic', categories: ['Coop', 'Unlisted'] }),
    game('3', { source: 'app' }),
  ];
  const counts = libraryCounts(games, ['Coop', 'Empty']);

  it('counts everything, favorites and each source', () => {
    expect(counts.all).toBe(3);
    expect(counts.favorites).toBe(1);
    expect(counts.steam).toBe(1);
    expect(counts.epic).toBe(1);
    expect(counts.app).toBe(1);
    expect(counts.gog).toBe(0);
  });

  it('counts listed categories only, including empty ones', () => {
    expect(counts['cat:Coop']).toBe(2);
    expect(counts['cat:Empty']).toBe(0);
    expect('cat:Unlisted' in counts).toBe(false);
  });
});

describe('visibleGames', () => {
  const games = [
    game('b', { name: 'Borderlands', favorite: true }),
    game('a', { name: 'Astroneer', source: 'epic', categories: ['Coop'] }),
    game('c', { name: 'Celeste', categories: ['Coop'] }),
  ];
  const names = (list: Game[]) => list.map((g) => g.name);

  it('shows everything by name on "all", and on "home" once a query is typed', () => {
    expect(names(visibleGames(games, 'all', '', 'name', {}))).toEqual(['Astroneer', 'Borderlands', 'Celeste']);
    expect(names(visibleGames(games, 'home', 'celes', 'name', {}))).toEqual(['Celeste']);
  });

  it('filters by favorites, by source and by exact category', () => {
    expect(names(visibleGames(games, 'favorites', '', 'name', {}))).toEqual(['Borderlands']);
    expect(names(visibleGames(games, 'epic', '', 'name', {}))).toEqual(['Astroneer']);
    expect(names(visibleGames(games, 'cat:Coop', '', 'name', {}))).toEqual(['Astroneer', 'Celeste']);
    expect(visibleGames(games, 'cat:coop', '', 'name', {})).toEqual([]);
  });

  it('orders by time played, then by name for entries never played', () => {
    const playtimes = { c: stat(7200), b: stat(60) };
    expect(names(visibleGames(games, 'all', '', 'played', playtimes))).toEqual(['Celeste', 'Borderlands', 'Astroneer']);
  });

  it('orders by last played, a missing date counting as never', () => {
    const playtimes = { a: stat(10, 100), b: stat(10, 900), c: stat(10) };
    expect(names(visibleGames(games, 'all', '', 'recent', playtimes))).toEqual(['Borderlands', 'Astroneer', 'Celeste']);
  });

  it('lets a query override the sort and stay inside the filter', () => {
    const playtimes = { a: stat(9999) };
    expect(names(visibleGames(games, 'cat:Coop', 'cel', 'played', playtimes))).toEqual(['Celeste']);
    expect(visibleGames(games, 'favorites', 'celeste', 'name', {})).toEqual([]);
  });

  it('treats a blank query as no query', () => {
    expect(visibleGames(games, 'all', '   ', 'name', {})).toHaveLength(3);
  });

  it('never reorders the list it was given', () => {
    const before = names(games);
    visibleGames(games, 'all', '', 'name', {});
    expect(names(games)).toEqual(before);
  });
});

describe('addCategories', () => {
  it('appends only names the entry does not have, ignoring case', () => {
    expect(addCategories(['Coop'], ['coop', 'Indie'])).toEqual(['Coop', 'Indie']);
  });

  it('does not add the same new name twice', () => {
    expect(addCategories([], ['Indie', 'INDIE'])).toEqual(['Indie']);
  });

  it('returns the same array when nothing is new, so callers can skip the write', () => {
    const current = ['Coop'];
    expect(addCategories(current, ['COOP'])).toBe(current);
  });

  it('accepts an entry with no categories yet', () => {
    expect(addCategories(undefined, ['Indie'])).toEqual(['Indie']);
  });
});

describe('planCategoryAdd', () => {
  const games = [
    game('1', { categories: ['Coop'] }),
    game('2', { categories: ['coop', 'Indie'] }),
    game('3'),
    game('4', { categories: undefined }),
  ];

  it('plans a write only for selected entries that gain a category', () => {
    const plan = planCategoryAdd(games, new Set(['1', '2', '4']), ['Coop', 'Indie']);
    expect([...plan.keys()]).toEqual(['1', '4']);
    expect(plan.get('1')).toEqual(['Coop', 'Indie']);
    expect(plan.get('4')).toEqual(['Coop', 'Indie']);
  });

  it('plans nothing when nothing is selected or nothing is added', () => {
    expect(planCategoryAdd(games, new Set(), ['Coop']).size).toBe(0);
    expect(planCategoryAdd(games, new Set(['1', '3']), []).size).toBe(0);
  });
});

describe('filterIsAlive', () => {
  it('only a category filter can die', () => {
    expect(filterIsAlive('home', [])).toBe(true);
    expect(filterIsAlive('steam', [])).toBe(true);
    expect(filterIsAlive('cat:Coop', ['Coop'])).toBe(true);
    expect(filterIsAlive('cat:Coop', ['Indie'])).toBe(false);
  });
});
