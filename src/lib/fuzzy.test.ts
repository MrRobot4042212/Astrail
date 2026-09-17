// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { describe, expect, it } from 'vitest';
import { fuzzyScore } from './fuzzy';

describe('fuzzyScore', () => {
  it('ranks a prefix above a mid-word substring', () => {
    const prefix = fuzzyScore('half', 'Half-Life');
    const middle = fuzzyScore('life', 'Half-Life');
    expect(prefix).not.toBeNull();
    expect(middle).not.toBeNull();
    expect(prefix!).toBeGreaterThan(middle!);
  });

  it('ranks a word start above a match inside a word', () => {
    const wordStart = fuzzyScore('life', 'Half Life');
    const inside = fuzzyScore('alf', 'Half Life');
    expect(wordStart!).toBeGreaterThan(inside!);
  });

  it('matches initials as a subsequence', () => {
    expect(fuzzyScore('hl', 'Half-Life')).not.toBeNull();
    expect(fuzzyScore('csgo', 'Counter Strike Global Offensive')).not.toBeNull();
  });

  it('rejects a query whose characters are out of order', () => {
    expect(fuzzyScore('zzz', 'Half-Life')).toBeNull();
    expect(fuzzyScore('efil', 'Half-Life')).toBeNull();
  });

  it('is case insensitive and ignores surrounding whitespace', () => {
    expect(fuzzyScore('  HALF ', 'half-life')).toBe(fuzzyScore('half', 'Half-Life'));
  });

  it('treats an empty query as "everything matches equally"', () => {
    expect(fuzzyScore('', 'Anything')).toBe(0);
    expect(fuzzyScore('   ', 'Anything')).toBe(0);
  });

  it('breaks ties in favour of the shorter title', () => {
    // Both are prefix matches, so without a tiebreaker the order was whatever
    // the library happened to be in.
    expect(fuzzyScore('portal', 'Portal')!).toBeGreaterThan(
      fuzzyScore('portal', 'Portal Knights')!,
    );
  });

  it('prefers the closer match when ranking a real library', () => {
    const games = ['Portal 2', 'Portal', 'Portal Knights', 'Teleport Simulator'];
    const ranked = games
      .map((name) => ({ name, score: fuzzyScore('portal', name) }))
      .filter((x) => x.score !== null)
      .sort((a, b) => b.score! - a.score!)
      .map((x) => x.name);
    expect(ranked[0]).toBe('Portal');
    expect(ranked).toContain('Portal Knights');
  });
});
