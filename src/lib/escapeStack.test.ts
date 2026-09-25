// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { describe, expect, it } from 'vitest';
import { dispatchEscape, escapeDepth, pushEscape } from './escapeStack';

describe('escape stack', () => {
  it('closes only the top-most layer', () => {
    // Regression: a confirmation over the settings closed both on one Escape.
    const closed: string[] = [];
    const removeSettings = pushEscape(() => closed.push('settings'));
    const removeConfirm = pushEscape(() => closed.push('confirm'));
    expect(dispatchEscape()).toBe(true);
    expect(closed).toEqual(['confirm']);

    removeConfirm();
    expect(dispatchEscape()).toBe(true);
    expect(closed).toEqual(['confirm', 'settings']);
    removeSettings();
    expect(escapeDepth()).toBe(0);
  });

  it('removing a lower layer keeps the order of the rest', () => {
    const closed: string[] = [];
    const a = pushEscape(() => closed.push('a'));
    const b = pushEscape(() => closed.push('b'));
    const c = pushEscape(() => closed.push('c'));
    b();
    dispatchEscape();
    c();
    dispatchEscape();
    a();
    expect(closed).toEqual(['c', 'a']);
  });

  it('does nothing when nothing is open', () => {
    expect(escapeDepth()).toBe(0);
    expect(dispatchEscape()).toBe(false);
  });

  it('removing twice is harmless', () => {
    const remove = pushEscape(() => {});
    remove();
    remove();
    expect(escapeDepth()).toBe(0);
  });
});
