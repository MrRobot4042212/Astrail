// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { describe, expect, it } from 'vitest';

import type { CoverAnswer, Game } from './types';
import {
  applyLedger,
  chunks,
  clearLedger,
  copyLedger,
  COVER_BATCH,
  coverKey,
  coverRecord,
  ICON_BATCH,
  iconKey,
  mergeScan,
  newLedger,
  nextRetry,
  pendingCovers,
  pendingIcons,
  RETRY_DELAYS_MS,
  sameGame,
  ScanClock,
  shouldStopPass,
  withArt,
} from './libraryState';

const game = (id: string, extra: Partial<Game> = {}): Game => ({
  id,
  name: `Game ${id}`,
  source: 'steam',
  favorite: false,
  categories: [],
  ...extra,
});

const app = (id: string, extra: Partial<Game> = {}): Game =>
  game(id, { name: `App ${id}`, source: 'app', executable: `C:\\Apps\\${id}.exe`, ...extra });

describe('what still has to be asked', () => {
  it('never asks IGDB about an app, nor for an icon of a game', () => {
    const list = [game('g'), app('a')];
    const ledger = newLedger();
    expect(pendingCovers(list, ledger).map((g) => g.id)).toEqual(['g']);
    expect(pendingIcons(list, ledger).map((g) => g.id)).toEqual(['a']);
  });

  it('skips entries that already carry art, and apps without an executable', () => {
    const list = [
      game('covered', { cover_url: 'c.jpg' }),
      app('user-cover', { cover_url: 'u.png' }),
      app('has-icon', { icon: 'i.png' }),
      app('no-exe', { executable: null }),
      game('blank', { name: '   ' }),
    ];
    const ledger = newLedger();
    expect(pendingCovers(list, ledger)).toEqual([]);
    expect(pendingIcons(list, ledger)).toEqual([]);
  });

  it('does not ask twice: answered questions, misses included, and duplicates', () => {
    const list = [
      game('hit'),
      game('miss'),
      game('dup-1', { name: 'Same Name' }),
      game('dup-2', { name: ' same name ' }),
      app('a1'),
      app('a2', { executable: 'C:\\APPS\\A1.EXE' }),
    ];
    const ledger = newLedger();
    ledger.covers.set(coverKey('Game hit'), 'hit.jpg');
    ledger.covers.set(coverKey('Game miss'), null);
    expect(pendingCovers(list, ledger).map((g) => g.id)).toEqual(['dup-1']);
    expect(pendingIcons(list, ledger).map((g) => g.id)).toEqual(['a1']);
  });

  it('asks the other question when an entry is reclassified', () => {
    // The old single set of ids answered "done" for both questions: an app
    // turned into a game never got a cover until restart.
    const ledger = newLedger();
    const asApp = app('x', { name: 'Thing' });
    ledger.icons.set(iconKey(asApp.executable as string), 'x.png');
    const asGame: Game = { ...asApp, source: 'windows' };
    expect(pendingCovers([asGame], ledger).map((g) => g.id)).toEqual(['x']);

    // And the other way round: a game already asked for a cover, now an app.
    const other = newLedger();
    other.covers.set(coverKey('Thing'), 'thing.jpg');
    expect(pendingIcons([asApp], other).map((g) => g.id)).toEqual(['x']);
  });

  it('asks again after a rename', () => {
    const ledger = newLedger();
    ledger.covers.set(coverKey('Old Name'), null);
    expect(pendingCovers([game('m', { name: 'New Name' })], ledger)).toHaveLength(1);
  });

  it('asks for everything again once cleared, and a copy is independent', () => {
    const ledger = newLedger();
    ledger.covers.set(coverKey('Game g'), 'g.jpg');
    const snapshot = copyLedger(ledger);
    clearLedger(ledger);
    expect(pendingCovers([game('g')], ledger)).toHaveLength(1);
    expect(pendingCovers([game('g')], snapshot)).toHaveLength(0);
  });
});

describe('dressing an entry with session art', () => {
  it('adds a resolved cover to a game and a resolved icon to an app', () => {
    const ledger = newLedger();
    ledger.covers.set(coverKey('Game g'), 'g.jpg');
    ledger.icons.set(iconKey('C:\\Apps\\a.exe'), 'a.png');
    expect(withArt(game('g'), ledger).cover_url).toBe('g.jpg');
    expect(withArt(app('a'), ledger).icon).toBe('a.png');
  });

  it('never overrides art the entry already carries', () => {
    const ledger = newLedger();
    ledger.covers.set(coverKey('Game g'), 'igdb.jpg');
    ledger.icons.set(iconKey('C:\\Apps\\a.exe'), 'a.png');
    const custom = game('g', { cover_url: 'user.png' });
    const covered = app('a', { cover_url: 'user.png' });
    expect(withArt(custom, ledger)).toBe(custom);
    expect(withArt(covered, ledger)).toBe(covered);
  });

  it('never gives an app an IGDB cover or a game an icon', () => {
    const ledger = newLedger();
    ledger.covers.set(coverKey('Brave'), 'the-film.jpg');
    ledger.icons.set(iconKey('C:\\Games\\g.exe'), 'g.png');
    const brave = app('b', { name: 'Brave' });
    const withExe = game('g', { executable: 'C:\\Games\\g.exe' });
    expect(withArt(brave, ledger).cover_url ?? null).toBeNull();
    expect(withArt(withExe, ledger).icon).toBeUndefined();
  });

  it('returns the same object when there is nothing to add', () => {
    const ledger = newLedger();
    ledger.covers.set(coverKey('Game g'), null);
    const g = game('g');
    expect(withArt(g, ledger)).toBe(g);
  });
});

describe('folding a scan into the list on screen', () => {
  it('keeps app icons across a scan', () => {
    // Regression: a plain refresh replaced the list with the scan result, which
    // never carries icons, while the ids stayed marked as answered. Every app
    // icon vanished until restart after toggling a type or renaming a category.
    const ledger = newLedger();
    ledger.icons.set(iconKey('C:\\Apps\\a.exe'), 'a.png');
    const prev = [app('a', { icon: 'a.png' }), game('g', { cover_url: 'g.jpg' })];
    const next = mergeScan(prev, [app('a'), game('g', { cover_url: 'g.jpg' })], ledger);
    expect(next[0].icon).toBe('a.png');
    expect(next).toBe(prev);
  });

  it('returns the same list for an unchanged library, and the same entries otherwise', () => {
    const ledger = newLedger();
    const prev = [game('a'), game('b'), game('c')];
    expect(mergeScan(prev, [game('a'), game('b'), game('c')], ledger)).toBe(prev);

    const next = mergeScan(prev, [game('a'), game('b', { favorite: true }), game('c')], ledger);
    expect(next).not.toBe(prev);
    expect(next[0]).toBe(prev[0]);
    expect(next[1]).not.toBe(prev[1]);
    expect(next[1].favorite).toBe(true);
    expect(next[2]).toBe(prev[2]);
  });

  it('picks up a changed field even when the set of ids is the same', () => {
    // Regression: the background refresh compared ids only, so a game moved to
    // another drive kept showing its old folder.
    const ledger = newLedger();
    const prev = [game('a', { install_dir: 'C:\\Old' })];
    const next = mergeScan(prev, [game('a', { install_dir: 'D:\\New' })], ledger);
    expect(next[0].install_dir).toBe('D:\\New');
  });

  it('follows the scan for membership and order', () => {
    const ledger = newLedger();
    const prev = [game('a'), game('b'), game('c')];
    const next = mergeScan(prev, [game('c'), game('new'), game('a')], ledger);
    expect(next.map((g) => g.id)).toEqual(['c', 'new', 'a']);
    expect(next[0]).toBe(prev[2]);
    expect(next[2]).toBe(prev[0]);

    const reordered = mergeScan(prev, [game('b'), game('a'), game('c')], ledger);
    expect(reordered).not.toBe(prev);
    expect(reordered.map((g) => g.id)).toEqual(['b', 'a', 'c']);
  });

  it('lets the scan override what is on screen, optimistic edits included', () => {
    const ledger = newLedger();
    ledger.covers.set(coverKey('Game a'), 'igdb.jpg');
    // The user removed a custom cover: the screen may still show it, the scan no
    // longer has it, and the session knows the IGDB one.
    const prev = [game('a', { cover_url: 'user.png', favorite: true })];
    const next = mergeScan(prev, [game('a')], ledger);
    expect(next[0].cover_url).toBe('igdb.jpg');
    expect(next[0].favorite).toBe(false);
  });

  it('drops covers that pointed into a wiped cache', () => {
    const ledger = newLedger();
    ledger.covers.set(coverKey('Game a'), 'covers/a.jpg');
    const prev = [game('a', { cover_url: 'covers/a.jpg' })];
    clearLedger(ledger);
    const scan = [game('a')];
    const next = mergeScan(prev, scan, ledger);
    expect(next[0].cover_url ?? null).toBeNull();
    expect(pendingCovers(scan, ledger)).toHaveLength(1);
  });

  it('moves a reclassified entry to the right kind of art', () => {
    const ledger = newLedger();
    ledger.covers.set(coverKey('Thing'), 'thing.jpg');
    ledger.icons.set(iconKey('C:\\Apps\\x.exe'), 'x.png');
    const asApp = app('x', { name: 'Thing' });
    const asGame: Game = { ...asApp, source: 'windows' };

    const toGame = mergeScan([{ ...asApp, icon: 'x.png' }], [asGame], ledger)[0];
    expect(toGame.cover_url).toBe('thing.jpg');
    expect(toGame.icon).toBeUndefined();

    const toApp = mergeScan([{ ...asGame, cover_url: 'thing.jpg' }], [asApp], ledger)[0];
    expect(toApp.cover_url ?? null).toBeNull();
    expect(toApp.icon).toBe('x.png');
  });

  it('paints a first scan over an empty list', () => {
    const scan = [game('a'), app('b')];
    expect(mergeScan([], scan, newLedger())).toEqual(scan);
    expect(mergeScan([], [], newLedger())).toEqual([]);
  });
});

describe('applying a batch of answers', () => {
  it('touches only the entries that gained art', () => {
    const ledger = newLedger();
    ledger.covers.set(coverKey('Game b'), 'b.jpg');
    const prev = [game('a'), game('b'), app('c')];
    const next = applyLedger(prev, ledger);
    expect(next[0]).toBe(prev[0]);
    expect(next[1].cover_url).toBe('b.jpg');
    expect(next[2]).toBe(prev[2]);
  });

  it('returns the same list when nothing on screen gains anything', () => {
    const ledger = newLedger();
    ledger.covers.set(coverKey('Gone'), 'gone.jpg');
    ledger.covers.set(coverKey('Game a'), null);
    const prev = [game('a')];
    expect(applyLedger(prev, ledger)).toBe(prev);
    expect(applyLedger(prev, newLedger())).toBe(prev);
  });

  it('shares one answer between entries with the same name', () => {
    const ledger = newLedger();
    ledger.covers.set(coverKey('Same'), 's.jpg');
    const next = applyLedger([game('1', { name: 'Same' }), game('2', { name: 'same ' })], ledger);
    expect(next.map((g) => g.cover_url)).toEqual(['s.jpg', 's.jpg']);
  });
});

describe('who owns the screen, and which art pass is alive', () => {
  it('a background scan does not touch the running pass until its result is applied', () => {
    // Regression: the background refresh cancelled the pass the moment it
    // started. If it then failed or found the same library, nothing restarted
    // the pass, and a splash waiting on it stayed up for good.
    const clock = new ScanClock();
    const boot = clock.claimScan();
    expect(clock.ownsScreen(boot)).toBe(true);
    const pass = clock.startPass();

    const background = clock.watchScan();
    expect(clock.isLive(pass)).toBe(true);
    expect(clock.isLatest(pass)).toBe(true);

    // ...the background scan comes back and is applied:
    expect(clock.ownsScreen(background)).toBe(true);
    const next = clock.startPass();
    expect(clock.isLive(pass)).toBe(false);
    expect(clock.isLatest(pass)).toBe(false);
    expect(clock.isLive(next)).toBe(true);
  });

  it('drops a scan overtaken by a newer refresh, in either order', () => {
    const clock = new ScanClock();
    const first = clock.claimScan();
    const background = clock.watchScan();
    const second = clock.claimScan();
    expect(clock.ownsScreen(first)).toBe(false);
    expect(clock.ownsScreen(background)).toBe(false);
    expect(clock.ownsScreen(second)).toBe(true);
    // A background scan started after the refresh may still land before it.
    expect(clock.ownsScreen(clock.watchScan())).toBe(true);
  });

  it('keeps an answer from a superseded pass, but none from before a cache wipe', () => {
    const clock = new ScanClock();
    const old = clock.startPass();
    const current = clock.startPass();
    expect(clock.isLive(old)).toBe(false);
    expect(clock.sameCache(old)).toBe(true);

    clock.wipe();
    expect(clock.sameCache(old)).toBe(false);
    expect(clock.sameCache(current)).toBe(false);
    // Stopped, yet still the pass that has to close the splash.
    expect(clock.isLive(current)).toBe(false);
    expect(clock.isLatest(current)).toBe(true);
    expect(clock.isLive(clock.startPass())).toBe(true);
  });
});

describe('entry equality', () => {
  it('treats null, undefined and a missing key alike, and compares categories by value', () => {
    const a = game('a', { cover_url: null, categories: ['x', 'y'] });
    const b = game('a', { icon: undefined, categories: ['x', 'y'] });
    expect(sameGame(a, b)).toBe(true);
    expect(sameGame(a, game('a', { categories: ['y', 'x'] }))).toBe(false);
    expect(sameGame(a, game('a', { categories: ['x', 'y'], launch_uri: 'steam://run/1' }))).toBe(false);
  });
});

describe('batched art passes', () => {
  const found = (path: string): CoverAnswer => ({ status: 'found', path });
  const miss: CoverAnswer = { status: 'not_found' };
  const offline: CoverAnswer = { status: 'unavailable' };

  it('cuts a list into chunks that keep the order and lose nothing', () => {
    expect(chunks([1, 2, 3, 4, 5], 2)).toEqual([[1, 2], [3, 4], [5]]);
    expect(chunks([], 8)).toEqual([]);
    expect(chunks([1, 2], 8)).toEqual([[1, 2]]);
    // A nonsense size must not loop forever or drop items.
    expect(chunks([1, 2, 3], 0)).toEqual([[1], [2], [3]]);
    expect(chunks([1, 2, 3], 1.9)).toEqual([[1], [2], [3]]);
  });

  it('stays under what one backend call accepts', () => {
    expect(COVER_BATCH).toBeLessThanOrEqual(64);
    expect(ICON_BATCH).toBeLessThanOrEqual(64);
  });

  it('records a path and a real miss, and nothing else', () => {
    expect(coverRecord(found('C:\\covers\\a.jpg'))).toBe('C:\\covers\\a.jpg');
    expect(coverRecord(miss)).toBeNull();
    expect(coverRecord(offline)).toBeUndefined();
    // A short answer list, an empty path, a status from a newer backend.
    expect(coverRecord(undefined)).toBeUndefined();
    expect(coverRecord(found(''))).toBeUndefined();
    expect(coverRecord({ status: 'throttled' } as unknown as CoverAnswer)).toBeUndefined();
  });

  it('asks again for a cover nobody could be asked about', () => {
    // Regression: "offline" used to arrive as `null` and was recorded as a miss,
    // so one start-up without network hid every missing cover for the session.
    const list = [game('a'), game('b'), game('c')];
    const ledger = newLedger();
    const answers = [found('C:\\covers\\a.jpg'), offline, miss];
    list.forEach((g, n) => {
      const value = coverRecord(answers[n]);
      if (value !== undefined) ledger.covers.set(coverKey(g.name), value);
    });
    expect(pendingCovers(list, ledger).map((g) => g.id)).toEqual(['b']);
  });

  it('stops a pass only when a whole chunk went unanswered', () => {
    expect(shouldStopPass([offline, offline])).toBe(true);
    expect(shouldStopPass([offline, miss])).toBe(false);
    expect(shouldStopPass([found('x'), offline])).toBe(false);
    // A failed call has no answers at all: the next chunk still gets its chance.
    expect(shouldStopPass([])).toBe(false);
  });

  it('does not retry when every name got an answer', () => {
    expect(nextRetry({ answered: 12, unavailable: 0 }, 2)).toEqual({ delayMs: null, fruitless: 0 });
    expect(nextRetry({ answered: 0, unavailable: 0 }, 0)).toEqual({ delayMs: null, fruitless: 0 });
  });

  it('backs off while nobody answers, then gives up until the next scan', () => {
    const nothing = { answered: 0, unavailable: 40 };
    let fruitless = 0;
    const delays: (number | null)[] = [];
    for (let n = 0; n < RETRY_DELAYS_MS.length + 1; n++) {
      const retry = nextRetry(nothing, fruitless);
      delays.push(retry.delayMs);
      fruitless = retry.fruitless;
    }
    expect(delays).toEqual([...RETRY_DELAYS_MS, null]);
    // And it stays given up, however often it is asked.
    expect(nextRetry(nothing, fruitless).delayMs).toBeNull();
  });

  it('starts the wait over once somebody answered', () => {
    expect(nextRetry({ answered: 3, unavailable: 5 }, 2)).toEqual({
      delayMs: RETRY_DELAYS_MS[0],
      fruitless: 0,
    });
    // A corrupt counter never indexes outside the table.
    expect(nextRetry({ answered: 0, unavailable: 1 }, -4).delayMs).toBe(RETRY_DELAYS_MS[0]);
    expect(nextRetry({ answered: 0, unavailable: 1 }, Number.NaN).delayMs).toBeNull();
  });
});
