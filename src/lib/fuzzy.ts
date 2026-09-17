// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

/** Lightweight fuzzy matcher (no deps). Returns a score for how well `query`
 *  matches `text`, or null if it doesn't match at all. Higher is better.
 *
 *  Scoring rewards: a contiguous substring hit (big bonus, extra if it's a word
 *  start), then a subsequence match with bonuses for consecutive chars and
 *  matches right after a separator (space/`-`/`_`/`:`). Empty query → score 0
 *  (everything matches equally). */
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.trim().toLowerCase();
  if (!q) return 0;
  const t = text.toLowerCase();

  // Fast path: direct substring is the strongest signal.
  const idx = t.indexOf(q);
  if (idx !== -1) {
    const wordStart = idx === 0 || /[\s\-_:.]/.test(t[idx - 1]);
    // Sub-point tiebreaker: with two equally good hits, prefer the shorter
    // title, so searching "portal" ranks Portal above Portal Knights. Always
    // < 1, so it can never outweigh the position/word-start bonuses.
    const brevity = Math.min(t.length, 100) / 100;
    return 1000 - idx + (wordStart ? 200 : 0) + q.length * 2 - brevity;
  }

  // Subsequence match with consecutive / word-boundary bonuses.
  let score = 0;
  let ti = 0;
  let consecutive = 0;
  for (let qi = 0; qi < q.length; qi++) {
    const c = q[qi];
    let found = -1;
    for (let j = ti; j < t.length; j++) {
      if (t[j] === c) {
        found = j;
        break;
      }
    }
    if (found === -1) return null; // a query char is missing → no match
    const afterSep = found === 0 || /[\s\-_:.]/.test(t[found - 1]);
    score += 1 + (afterSep ? 8 : 0) + consecutive * 3;
    consecutive = found === ti ? consecutive + 1 : 0;
    ti = found + 1;
  }
  return score;
}
