// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import type { Game, Category, CoverAnswer, PlayStat } from '@/lib/types';
import {
  getLibrary,
  cachedLibrary,
  resolveCovers,
  listCategories,
  appIcons,
  allPlaytime,
  libraryChanged,
  mainWindowVisible,
} from '@/lib/tauri';
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
  ScanClock,
  shouldStopPass,
  type PassOutcome,
  type PassToken,
} from '@/lib/libraryState';

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
  // Which scan owns the screen and which art pass is alive (rules and their
  // history in `libraryState.ts`).
  const clock = useRef(new ScanClock());
  // Mirror of `booting` for code that runs outside render.
  const splashOn = useRef(false);
  // What the backend has answered this session (see `libraryState.ts`), so a
  // refresh does not re-ask for art it already has an answer for.
  const ledger = useRef(newLedger());

  // --- Batched art application ---------------------------------------------
  // The cover pass resolves hundreds of entries; applying each one with its own
  // `setGames` produced one React commit per item (and, with the grid rendered
  // from this state, a full filter+sort each time). Answers go into the ledger
  // and the list is re-dressed from it once per batch.
  const artDirty = useRef(false);
  const flushTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const flushArt = useCallback(() => {
    if (flushTimer.current !== null) {
      clearTimeout(flushTimer.current);
      flushTimer.current = null;
    }
    if (!artDirty.current) return;
    artDirty.current = false;
    // A snapshot, so the updater does not read a ref that keeps changing.
    const answers = copyLedger(ledger.current);
    setGames((prev) => applyLedger(prev, answers));
  }, []);

  const recordArt = useCallback(
    (kind: 'covers' | 'icons', key: string, value: string | null) => {
      ledger.current[kind].set(key, value);
      if (!value) return;
      artDirty.current = true;
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

  const endSplash = useCallback(() => {
    booted.current = true;
    splashOn.current = false;
    setBooting(false);
  }, []);

  // One pass over the entries that still need a cover, one IPC call per chunk.
  // Concurrency and IGDB's rate limit are the backend's business (`resolve_covers`).
  const coverPass = useCallback(
    async (list: Game[], pass: PassToken): Promise<PassOutcome> => {
      const live = () => clock.current.isLive(pass);
      const pending = pendingCovers(list, ledger.current);
      const total = pending.length;
      const outcome: PassOutcome = { answered: 0, unavailable: 0 };
      // The progress bar only exists while the splash is up.
      if (splashOn.current) setCoverProgress({ done: 0, total });
      let done = 0;

      for (const chunk of chunks(pending, COVER_BATCH)) {
        if (!live()) break; // a newer scan took over, or the cache was wiped
        let answers: CoverAnswer[] = [];
        try {
          answers = await resolveCovers(chunk.map((g) => g.name));
        } catch {
          // Nothing recorded, so these names are asked again; one failed call
          // does not break the rest.
        }
        chunk.forEach((game, n) => {
          const value = coverRecord(answers[n]);
          if (value === undefined) {
            // Nobody could be asked. That is not an answer and is never recorded:
            // it used to arrive as `null`, which made an offline start mark every
            // cover as missing for the whole session.
            outcome.unavailable++;
            return;
          }
          outcome.answered++;
          // Recorded even if a newer pass has taken over meanwhile: the answer is
          // about the name, not about the run. A miss is recorded too: Rust caches
          // it for days, so asking again on the next refresh only burns IPC.
          if (clock.current.sameCache(pass)) recordArt('covers', coverKey(game.name), value);
        });
        done += chunk.length;
        if (live() && splashOn.current) setCoverProgress({ done, total });
        if (shouldStopPass(answers)) {
          // A whole chunk without an answer: the service is unreachable, so the
          // rest would only queue up behind the same timeouts.
          outcome.unavailable += total - done;
          break;
        }
      }

      flushArt();
      return outcome;
    },
    [flushArt, recordArt],
  );

  // The cover pass that is waiting to ask again, and how many passes in a row got
  // no answer at all (rules in `libraryState.ts::nextRetry`).
  const coverRetry = useRef<{ timer: ReturnType<typeof setTimeout>; cancel: () => void } | null>(
    null,
  );
  const fruitless = useRef(0);
  const disposed = useRef(false);

  const cancelCoverRetry = useCallback(() => {
    const waiting = coverRetry.current;
    if (waiting === null) return;
    coverRetry.current = null;
    clearTimeout(waiting.timer);
    waiting.cancel();
  }, []);

  /** Resolves `true` when the delay elapsed, `false` when it was cancelled. */
  const waitForRetry = useCallback(
    (delayMs: number) =>
      new Promise<boolean>((resolve) => {
        const timer = setTimeout(() => {
          coverRetry.current = null;
          resolve(true);
        }, delayMs);
        coverRetry.current = { timer, cancel: () => resolve(false) };
      }),
    [],
  );

  useEffect(() => {
    disposed.current = false;
    return () => {
      disposed.current = true;
      cancelCoverRetry();
    };
  }, [cancelCoverRetry]);

  // The cover pass, and then again for whatever could not be asked (offline at
  // start-up, IGDB rate limit). Nothing else would ask: the periodic refresh only
  // scans when a store changed.
  const resolveCoversFor = useCallback(
    async (list: Game[], pass: PassToken) => {
      let outcome: PassOutcome;
      try {
        outcome = await coverPass(list, pass);
      } finally {
        // The latest pass closes the splash, whichever scan opened it. Only the
        // first pass does: a retry runs long after the splash is gone.
        if (clock.current.isLatest(pass)) endSplash();
      }
      while (!disposed.current && clock.current.isLive(pass)) {
        const retry = nextRetry(outcome, fruitless.current);
        fruitless.current = retry.fruitless;
        if (retry.delayMs === null) return;
        if (!(await waitForRetry(retry.delayMs))) return;
        if (disposed.current || !clock.current.isLive(pass)) return;
        outcome = await coverPass(list, pass);
      }
    },
    [coverPass, endSplash, waitForRetry],
  );

  // Resolve real exe icons for apps that have no cover and no known brand logo.
  // Asked by id: the backend looks the executable up itself, the webview never
  // names a path.
  const resolveIconsFor = useCallback(
    async (list: Game[], pass: PassToken) => {
      const pending = pendingIcons(list, ledger.current);
      for (const chunk of chunks(pending, ICON_BATCH)) {
        if (!clock.current.isLive(pass)) break;
        try {
          const icons = await appIcons(chunk.map((g) => g.id));
          if (!clock.current.sameCache(pass)) break;
          chunk.forEach((game, n) => {
            // `pendingIcons` only returns entries with an executable.
            recordArt('icons', iconKey(game.executable as string), icons[n] ?? null);
          });
        } catch {
          // No icon is fine: the card falls back to the letter placeholder.
        }
      }
      flushArt();
    },
    [flushArt, recordArt],
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
   * Forget what the backend has answered, so the next pass asks again.
   *
   * The ledger exists to stop a routine refresh re-asking the backend for art it
   * already answered. It was only ever added to, which quietly broke the one
   * feature whose entire purpose is re-downloading art: after "vaciar caché de
   * portadas" everything was already answered, so the pending list came out
   * empty and not a single cover was fetched until the app restarted.
   *
   * Callers follow this with `refresh()`: the scan is what drops the covers
   * that pointed at the deleted files.
   */
  const resetArt = useCallback(() => {
    clearLedger(ledger.current);
    clock.current.wipe();
    artDirty.current = false;
    cancelCoverRetry();
  }, [cancelCoverRetry]);

  // Put a scan result on screen and (re)start the art passes for it.
  const applyScan = useCallback(
    (list: Game[]) => {
      const pass = clock.current.startPass();
      // A pass waiting to ask again belongs to the list this one replaces.
      cancelCoverRetry();
      fruitless.current = 0;
      const answers = copyLedger(ledger.current);
      setGames((prev) => mergeScan(prev, list, answers));
      setLoading(false);
      // A background scan that succeeds makes an earlier scan error stale.
      setError(null);
      // Both passes filter on the ledger, so starting them for a list that needs
      // nothing costs two array walks. Starting them every time is what
      // guarantees the pass cancelled above has a successor.
      resolveIconsFor(list, pass);
      resolveCoversFor(list, pass);
    },
    [cancelCoverRetry, resolveCoversFor, resolveIconsFor],
  );

  const refresh = useCallback(
    async (showSplash = false) => {
      const myScan = clock.current.claimScan();
      // An explicit re-scan is a request to redo the work, not to reuse what this
      // session happens to remember.
      if (showSplash) clearLedger(ledger.current);
      // Splash shows on the very first run, or whenever explicitly requested
      // (e.g. the "volver a escanear" button) so a re-scan feels like a reload.
      const splash = showSplash || !booted.current;
      if (splash) {
        splashOn.current = true;
        setBooting(true);
        setCoverProgress({ done: 0, total: 0 });
      }
      setLoading(true);
      setError(null);
      try {
        const list = mockCount > 0 ? mockLibrary() : await getLibrary();
        if (!clock.current.ownsScreen(myScan)) return; // a newer refresh took over
        applyScan(list);
      } catch (e) {
        if (!clock.current.ownsScreen(myScan)) return;
        setError(String(e));
        setLoading(false);
        endSplash();
      }
    },
    [applyScan, endSplash, mockCount, mockLibrary],
  );

  const silentRefresh = useCallback(async () => {
    // Does not claim a scan number: if a `refresh` starts while this scan is
    // running, that one wins and this result is dropped.
    const seen = clock.current.watchScan();
    try {
      const list = await getLibrary();
      if (!clock.current.ownsScreen(seen)) return;
      applyScan(list);
    } catch {
      // Background work: keep what is on screen, and the pass in flight.
    }
  }, [applyScan]);

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

  // A backup was imported (Settings → "Copia de tus datos"): manual apps, hidden
  // entries, favorites, categories and chosen covers all changed on disk at once.
  // The scan is what applies them; play time arrives through `playtime-updated`.
  useEffect(() => {
    const un = listen('user-data-imported', () => {
      refresh();
      refreshCategories();
    });
    return () => {
      un.then((f) => f());
    };
  }, [refresh, refreshCategories]);

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
    // A start from autostart stays in the tray and announces it before this
    // listener exists, so the initial state is asked once; any event that
    // arrives first is newer and wins.
    let heard = false;
    mainWindowVisible()
      .then((v) => {
        if (!heard && !v) {
          visible = false;
          missedWhileHidden = true;
          stopTimer();
        }
      })
      .catch(() => {});
    const un = listen<boolean>('window-visibility', (event) => {
      heard = true;
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
