// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

// The library list's state transitions, without React or IPC, so they can be
// tested. `useLibrary` owns the timers and the calls; every decision about
// *what the list becomes*, and about which scan or art pass still counts, is
// made here.

import type { CoverAnswer, Game } from './types';

/**
 * What the backend has answered this session, keyed by the question asked.
 *
 * A cover is asked by *name* and an icon is read from an *executable*, so those
 * are the keys — not the entry id. A set of ids could not tell "asked for a
 * cover" from "asked for an icon" (an app turned into a game was never asked
 * for a cover), went stale when an entry was renamed, and remembered only
 * *that* something was answered, not *what*: a scan that replaced the list lost
 * every icon, and nothing asked for them again until restart.
 *
 * `null` is an answer too ("there is none"), so a miss is not asked twice. "Could
 * not ask" is **not** an answer and never gets in here (see `coverRecord`).
 */
export interface ArtLedger {
  covers: Map<string, string | null>;
  icons: Map<string, string | null>;
}

export function newLedger(): ArtLedger {
  return { covers: new Map(), icons: new Map() };
}

export function copyLedger(ledger: ArtLedger): ArtLedger {
  return { covers: new Map(ledger.covers), icons: new Map(ledger.icons) };
}

export function clearLedger(ledger: ArtLedger): void {
  ledger.covers.clear();
  ledger.icons.clear();
}

/** An art pass, as issued by `ScanClock.startPass`. */
export interface PassToken {
  run: number;
  epoch: number;
}

/**
 * Who owns the screen and which art pass is alive. Three counters whose rules
 * were wrong when they were one:
 *
 * - *scan*: claimed by a foreground refresh when it starts, so an older scan
 *   returning late is dropped. A background scan only watches it.
 * - *run*: bumped when a scan result is **applied**, never when a scan merely
 *   starts. A scan that fails, or brings nothing new, must not cancel the pass
 *   in flight: nothing would restart it, and a splash waiting on it never closed.
 * - *epoch*: bumped when the cover cache is wiped. An answer computed before
 *   the wipe points at a deleted file.
 */
export class ScanClock {
  private scan = 0;
  private run = 0;
  private epoch = 0;

  /** A foreground refresh starts: every scan already in flight loses the screen. */
  claimScan(): number {
    return ++this.scan;
  }

  /** A background scan starts: it yields to any refresh that starts after it. */
  watchScan(): number {
    return this.scan;
  }

  /** May the scan holding `token` put its result on screen? */
  ownsScreen(token: number): boolean {
    return this.scan === token;
  }

  /** A scan result is being applied: the previous pass ends here. */
  startPass(): PassToken {
    return { run: ++this.run, epoch: this.epoch };
  }

  /** Should the pass keep asking? */
  isLive(pass: PassToken): boolean {
    return this.isLatest(pass) && this.sameCache(pass);
  }

  /** Is this the pass the splash is waiting for? True even after a wipe stopped
   *  it: it still has to close the splash. */
  isLatest(pass: PassToken): boolean {
    return pass.run === this.run;
  }

  /** May an answer this pass just received be recorded? True for a superseded
   *  pass too: the answer is about a name, not about the run. */
  sameCache(pass: PassToken): boolean {
    return pass.epoch === this.epoch;
  }

  wipe(): void {
    this.epoch++;
  }
}

/** Same normalisation the cover cache uses for its own key. */
export function coverKey(name: string): string {
  return name.trim().toLowerCase();
}

/** Windows paths are case-insensitive. */
export function iconKey(executable: string): string {
  return executable.trim().toLowerCase();
}

/** IGDB is a games database: asking it about an app returns wrong art (the
 *  Brave browser gets the film "Brave"). Apps use their exe icon instead. */
export function wantsCover(g: Game): boolean {
  return g.source !== 'app' && !g.cover_url && coverKey(g.name) !== '';
}

export function wantsIcon(g: Game): boolean {
  return g.source === 'app' && !g.cover_url && !g.icon && !!g.executable;
}

/** Entries to ask a cover for: unanswered, one per name. */
export function pendingCovers(list: readonly Game[], ledger: ArtLedger): Game[] {
  const seen = new Set<string>();
  return list.filter((g) => {
    if (!wantsCover(g)) return false;
    const key = coverKey(g.name);
    if (ledger.covers.has(key) || seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

/** Apps to extract an icon for: unanswered, one per executable. */
export function pendingIcons(list: readonly Game[], ledger: ArtLedger): Game[] {
  const seen = new Set<string>();
  return list.filter((g) => {
    if (!wantsIcon(g)) return false;
    const key = iconKey(g.executable as string);
    if (ledger.icons.has(key) || seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

/**
 * An entry with whatever this session knows about its art. Art the entry
 * already carries (a user cover, a cover the scan found on disk) always wins.
 * Returns the same object when there is nothing to add.
 */
export function withArt(g: Game, ledger: ArtLedger): Game {
  if (g.source === 'app') {
    if (!wantsIcon(g)) return g;
    const icon = ledger.icons.get(iconKey(g.executable as string));
    return icon ? { ...g, icon } : g;
  }
  if (!wantsCover(g)) return g;
  const cover = ledger.covers.get(coverKey(g.name));
  return cover ? { ...g, cover_url: cover } : g;
}

function sameValue(a: unknown, b: unknown): boolean {
  if (a == null && b == null) return true;
  if (Array.isArray(a) && Array.isArray(b)) {
    return a.length === b.length && a.every((v, i) => v === b[i]);
  }
  return a === b;
}

/** Field-by-field equality over whatever keys the two entries have, so a field
 *  added to `Game` later is compared without touching this. */
export function sameGame(a: Game, b: Game): boolean {
  if (a === b) return true;
  const keys = new Set([...Object.keys(a), ...Object.keys(b)]) as Set<keyof Game>;
  for (const k of keys) {
    if (!sameValue(a[k], b[k])) return false;
  }
  return true;
}

/**
 * Fold a scan result into the list on screen. The scan decides membership,
 * order and every backend field; the ledger adds the art the scan cannot know.
 * Nothing is inherited from the list on screen, so an optimistic edit the
 * backend rejected, or a cover the user just removed, never survives a scan.
 *
 * Unchanged entries keep their object identity (memoized cards do not
 * re-render), and an unchanged library returns `prev` itself.
 */
export function mergeScan(prev: readonly Game[], scan: readonly Game[], ledger: ArtLedger): Game[] {
  const byId = new Map(prev.map((g) => [g.id, g]));
  let changed = prev.length !== scan.length;
  const next = scan.map((g, i) => {
    const dressed = withArt(g, ledger);
    const old = byId.get(g.id);
    if (old && sameGame(old, dressed)) {
      if (prev[i] !== old) changed = true;
      return old;
    }
    changed = true;
    return dressed;
  });
  return changed ? next : (prev as Game[]);
}

/** Apply newly answered art to the list on screen. Same identity rules. */
export function applyLedger(prev: readonly Game[], ledger: ArtLedger): Game[] {
  let changed = false;
  const next = prev.map((g) => {
    const dressed = withArt(g, ledger);
    if (dressed !== g) changed = true;
    return dressed;
  });
  return changed ? next : (prev as Game[]);
}

// --- Batched art passes ------------------------------------------------------
// Art is asked for in chunks (`resolve_covers`, `app_icons`): one IPC round trip
// per chunk instead of one per entry. The backend refuses more than 64 per call.

/** Names per `resolve_covers` call. The backend works a chunk on four threads
 *  (IGDB's in-flight cap), so this is about two rounds of lookups: small enough
 *  that the splash progress moves about once a second, large enough that one slow
 *  lookup does not idle the other three for long. Estimated, not measured. */
export const COVER_BATCH = 8;
/** Ids per `app_icons` call. Local work (a PE parse each), so larger chunks. */
export const ICON_BATCH = 16;

export function chunks<T>(list: readonly T[], size: number): T[][] {
  const step = Math.max(1, Math.floor(size));
  const out: T[][] = [];
  for (let i = 0; i < list.length; i += step) out.push(list.slice(i, i + step));
  return out;
}

/**
 * What a cover answer means for the ledger: a path, `null` for a real miss, or
 * `undefined` for "do not record".
 *
 * The backend used to answer `string | null`, which folded "IGDB has no cover"
 * and "IGDB could not be asked" into the same `null`. Both were recorded, so one
 * start-up without network marked every missing cover as answered and nothing
 * asked again until a restart or a manual re-scan. An answer this build does not
 * know is treated as unasked, never as a miss.
 */
export function coverRecord(answer: CoverAnswer | undefined): string | null | undefined {
  if (answer?.status === 'found') return answer.path || undefined;
  if (answer?.status === 'not_found') return null;
  return undefined;
}

/** How a cover pass went: names that got an answer, and names nobody could be
 *  asked about. */
export interface PassOutcome {
  answered: number;
  unavailable: number;
}

/** A whole chunk came back "could not ask": the network (or IGDB) is down, and
 *  the rest of the pass would only queue the same failure. */
export function shouldStopPass(answers: readonly (CoverAnswer | undefined)[]): boolean {
  return answers.length > 0 && answers.every((a) => coverRecord(a) === undefined);
}

/** Wait before asking again for what could not be asked, by how many passes in a
 *  row got no answer at all. After the last one the pass gives up until the next
 *  scan: a build without IGDB credentials answers "unavailable" forever. */
export const RETRY_DELAYS_MS: readonly number[] = [60_000, 5 * 60_000, 15 * 60_000];

/**
 * Whether (and when) to run the cover pass again, and the new count of fruitless
 * passes in a row. A pass that got at least one answer proves somebody is
 * answering, so the wait starts over from the shortest delay.
 */
export function nextRetry(
  outcome: PassOutcome,
  fruitless: number,
): { delayMs: number | null; fruitless: number } {
  if (outcome.unavailable <= 0) return { delayMs: null, fruitless: 0 };
  if (outcome.answered > 0) return { delayMs: RETRY_DELAYS_MS[0], fruitless: 0 };
  const count = Math.max(0, Math.floor(fruitless));
  return { delayMs: RETRY_DELAYS_MS[count] ?? null, fruitless: count + 1 };
}
