// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import type { Game, Category, PlayStat } from '@/lib/types';
import {
  getLibrary,
  cachedLibrary,
  resolveCover,
  listCategories,
  appIcon,
  allPlaytime,
  libraryChanged,
} from '@/lib/tauri';

/** Resolve covers a few at a time to stay under IGDB's ~4 req/s rate limit. */
const COVER_CONCURRENCY = 3;
/** How often to look for newly installed/removed games. */
const REFRESH_INTERVAL_MS = 15 * 60 * 1000;
/** Art results are applied in batches this often, instead of one render each. */
const ART_FLUSH_MS = 120;

// `autoScan` gates the very first library scan. Returning users pass `true` so it
// runs in the background on open; first-run users pass `false` until they finish
// onboarding and hit "Escanear" — that way the slow native scan never freezes the
// onboarding screen, and its splash is a deliberate, user-triggered step.
export function useLibrary(autoScan: boolean) {
  const [games, setGames] = useState<Game[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  // Explicitly-created categories with icons (persist even with zero games).
  const [categoryMeta, setCategoryMeta] = useState<Category[]>([]);
  // Play stats per game id, for sorting by played/recent (live-updated).
  const [playtimes, setPlaytimes] = useState<Record<string, PlayStat>>({});
  // First-run splash: turned on by the first splash-worthy `refresh` and off when
  // its cover pass finishes. `coverProgress` drives the splash's progress bar.
  const [booting, setBooting] = useState(false);
  const [coverProgress, setCoverProgress] = useState({ done: 0, total: 0 });
  // Flips true once we've checked the on-disk cache, so the deferred scan always
  // starts *after* the instant cache paint (which sets `booted` → no splash flash).
  const [cacheChecked, setCacheChecked] = useState(false);
  const booted = useRef(false);
  // Ensures the initial scan fires at most once.
  const started = useRef(false);
  // Bumped on each refresh so a stale in-flight cover pass can bail out.
  const runId = useRef(0);
  // Ids already resolved (or confirmed coverless) this session, so a refresh does
  // not re-ask the backend for art it has already answered.
  const artDone = useRef(new Set<string>());

  // --- Batched art application ---------------------------------------------
  // The cover pass resolves hundreds of entries; applying each one with its own
  // `setGames` produced one React commit per item (and, with the grid rendered
  // from this state, a full filter+sort each time). Results are collected here
  // and flushed together.
  const pendingArt = useRef(new Map<string, Partial<Game>>());
  const flushTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const flushArt = useCallback(() => {
    flushTimer.current = null;
    if (pendingArt.current.size === 0) return;
    const batch = pendingArt.current;
    pendingArt.current = new Map();
    setGames((prev) => prev.map((g) => (batch.has(g.id) ? { ...g, ...batch.get(g.id) } : g)));
  }, []);

  const queueArt = useCallback(
    (id: string, patch: Partial<Game>) => {
      pendingArt.current.set(id, { ...pendingArt.current.get(id), ...patch });
      if (flushTimer.current === null) {
        flushTimer.current = setTimeout(flushArt, ART_FLUSH_MS);
      }
    },
    [flushArt],
  );

  useEffect(
    () => () => {
      if (flushTimer.current !== null) clearTimeout(flushTimer.current);
    },
    [],
  );

  const resolveCovers = useCallback(
    async (list: Game[], myRun: number) => {
      // IGDB is a *games* database, so resolving covers for apps yields wrong art
      // (e.g. the Brave browser → the movie "Brave"). Apps use their exe icon
      // instead; only fetch covers for entries that still lack one and aren't apps.
      const pending = list.filter(
        (g) => !g.cover_url && g.source !== 'app' && !artDone.current.has(g.id),
      );
      const total = pending.length;
      // The progress bar only exists during the first-run splash.
      if (booted.current === false) setCoverProgress({ done: 0, total });
      let i = 0;
      let done = 0;

      const worker = async () => {
        while (i < pending.length) {
          const game = pending[i++];
          if (runId.current !== myRun) return; // a newer refresh superseded us
          try {
            const url = await resolveCover(game.name);
            if (url && runId.current === myRun) queueArt(game.id, { cover_url: url });
            // Remember the answer either way: a miss is cached in Rust for days,
            // so asking again on the next refresh only burns IPC round-trips.
            artDone.current.add(game.id);
          } catch {
            // Leave the placeholder; one missing cover shouldn't break the rest.
          } finally {
            done++;
            if (runId.current === myRun && booted.current === false) {
              setCoverProgress({ done, total });
            }
          }
        }
      };

      await Promise.all(Array.from({ length: Math.min(COVER_CONCURRENCY, total) }, worker));
      flushArt();
    },
    [flushArt, queueArt],
  );

  // Resolve real exe icons for apps that have no cover and no known brand logo.
  // Local extraction (no network/rate limit), so a higher concurrency is fine.
  const resolveIcons = useCallback(
    async (list: Game[], myRun: number) => {
      const pending = list.filter(
        (g) =>
          g.source === 'app' &&
          !g.cover_url &&
          !g.icon &&
          g.executable &&
          !artDone.current.has(g.id),
      );
      let i = 0;
      const worker = async () => {
        while (i < pending.length) {
          const game = pending[i++];
          if (runId.current !== myRun) return;
          try {
            const path = await appIcon(game.executable as string);
            if (path && runId.current === myRun) queueArt(game.id, { icon: path });
            artDone.current.add(game.id);
          } catch {
            // No icon is fine: the card falls back to the letter placeholder.
          }
        }
      };
      await Promise.all(Array.from({ length: Math.min(6, pending.length) }, worker));
      flushArt();
    },
    [flushArt, queueArt],
  );

  // Dev-only fixture for profiling the grid at realistic sizes:
  // `NEXT_PUBLIC_MOCK_LIBRARY=500 npm run dev`. The value is inlined at build
  // time, so this whole branch is dead code (and dropped) in a release build.
  const mockCount = Number(process.env.NEXT_PUBLIC_MOCK_LIBRARY ?? 0);
  const mockLibrary = useCallback((): Game[] => {
    const sources: Game['source'][] = ['steam', 'epic', 'gog', 'xbox', 'app'];
    return Array.from({ length: mockCount }, (_, i) => ({
      id: `mock:${i}`,
      name: `Mock Game ${String(i).padStart(4, '0')}`,
      source: sources[i % sources.length],
      favorite: i % 17 === 0,
      categories: i % 5 === 0 ? ['Mock'] : [],
    }));
  }, [mockCount]);

  /**
   * Forget which entries have already been asked about, so the next pass asks
   * again.
   *
   * `artDone` exists to stop a routine refresh re-asking the backend for art it
   * already answered. It was only ever added to, which quietly broke the one
   * feature whose entire purpose is re-downloading art: after "vaciar caché de
   * portadas" every id was already in the set, so the pending list came out empty
   * and not a single cover was fetched until the app restarted.
   */
  const resetArt = useCallback(() => {
    artDone.current.clear();
  }, []);

  const refresh = useCallback(
    async (showSplash = false) => {
      const myRun = ++runId.current;
      // An explicit re-scan is a request to redo the work, not to reuse what this
      // session happens to remember.
      if (showSplash) artDone.current.clear();
      // Splash shows on the very first run, or whenever explicitly requested
      // (e.g. the "volver a escanear" button) so a re-scan feels like a reload.
      const splash = showSplash || !booted.current;
      if (splash) {
        setBooting(true);
        setCoverProgress({ done: 0, total: 0 });
      }
      setLoading(true);
      setError(null);
      try {
        const list = mockCount > 0 ? mockLibrary() : await getLibrary();
        if (runId.current !== myRun) return;
        setGames(list);
        setLoading(false);
        resolveIcons(list, myRun);
        const pass = resolveCovers(list, myRun);
        if (splash) {
          pass.finally(() => {
            if (runId.current === myRun) {
              booted.current = true;
              setBooting(false);
            }
          });
        }
      } catch (e) {
        if (runId.current === myRun) {
          setError(String(e));
          setLoading(false);
          booted.current = true;
          setBooting(false);
        }
      }
    },
    [mockCount, mockLibrary, resolveCovers, resolveIcons],
  );

  const silentRefresh = useCallback(async () => {
    const myRun = ++runId.current;
    try {
      const list = await getLibrary();
      if (runId.current !== myRun) return;

      setGames((prev) => {
        // Index once instead of scanning `prev` per entry (this was O(n²)).
        const byId = new Map(prev.map((g) => [g.id, g]));
        const sameSet =
          prev.length === list.length && list.every((g) => byId.has(g.id));
        if (sameSet) return prev;

        // Preserve existing covers/icons to prevent blinking
        const mergedList = list.map((newGame) => {
          const oldGame = byId.get(newGame.id);
          return oldGame
            ? { ...newGame, cover_url: newGame.cover_url ?? oldGame.cover_url, icon: oldGame.icon }
            : newGame;
        });

        // Resolve art for any brand new games in the background
        setTimeout(() => {
          resolveIcons(mergedList, myRun);
          resolveCovers(mergedList, myRun);
        }, 0);

        return mergedList;
      });
    } catch {
      // Fail silently in background
    }
  }, [resolveCovers, resolveIcons]);

  const refreshCategories = useCallback(async () => {
    try {
      setCategoryMeta(await listCategories());
    } catch {
      // Non-fatal: the sidebar just won't show empty categories.
    }
  }, []);

  const refreshPlaytimes = useCallback(async () => {
    try {
      setPlaytimes(await allPlaytime());
    } catch {
      // Non-fatal: sorting by playtime just falls back to zeros.
    }
  }, []);

  // Keep play stats fresh: reload when the global watcher closes a session.
  useEffect(() => {
    const un = listen('playtime-updated', () => refreshPlaytimes());
    return () => {
      un.then((f) => f());
    };
  }, [refreshPlaytimes]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      // Paint the last cached library instantly (no splash). The actual scan is
      // deferred to the effect below so it can wait on `autoScan`.
      try {
        const cache = await cachedLibrary();
        if (!cancelled && cache.length) {
          setGames(cache);
          setLoading(false);
          booted.current = true;
        }
      } catch {
        // No cache: the deferred scan will show the first-run splash.
      }
      if (!cancelled) setCacheChecked(true);
    })();
    refreshCategories();
    refreshPlaytimes();
    return () => {
      cancelled = true;
    };
  }, [refreshCategories, refreshPlaytimes]);

  // Fire the initial scan once the cache is painted and scanning is allowed
  // (immediately for returning users, after onboarding for first-run users).
  useEffect(() => {
    if (cacheChecked && autoScan && !started.current) {
      started.current = true;
      refresh();
    }
  }, [cacheChecked, autoScan, refresh]);

  // Periodic check for installs/uninstalls.
  //
  // Two guards, because this used to run a full native scan (8 scanners + a
  // PowerShell AppX enumeration) every 15 minutes even with the window hidden in
  // the tray:
  //   * `window-visibility` from Rust pauses the timer while nobody can see the
  //     result (`document.hidden` is unreliable when only the HWND is hidden);
  //   * `libraryChanged()` fingerprints the stores first, so a scan only happens
  //     when something was actually installed or removed.
  useEffect(() => {
    if (!autoScan) return;
    let visible = true;
    let timer: ReturnType<typeof setInterval> | null = null;
    let missedWhileHidden = false;

    const tick = async () => {
      try {
        if (!(await libraryChanged())) return;
      } catch {
        // Fingerprint unavailable → fall through to the scan.
      }
      silentRefresh();
    };

    const startTimer = () => {
      if (timer === null) timer = setInterval(tick, REFRESH_INTERVAL_MS);
    };
    const stopTimer = () => {
      if (timer !== null) {
        clearInterval(timer);
        timer = null;
      }
    };

    startTimer();
    const un = listen<boolean>('window-visibility', (event) => {
      visible = event.payload;
      if (visible) {
        startTimer();
        if (missedWhileHidden) {
          missedWhileHidden = false;
          tick();
        }
      } else {
        missedWhileHidden = true;
        stopTimer();
      }
    });

    return () => {
      stopTimer();
      un.then((f) => f());
    };
  }, [autoScan, silentRefresh]);

  return {
    games,
    loading,
    error,
    playtimes,
    refresh,
    resetArt,
    silentRefresh,
    setGames,
    categoryMeta,
    refreshCategories,
    booting,
    coverProgress,
  };
}
