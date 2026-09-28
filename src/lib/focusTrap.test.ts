// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { describe, expect, it } from 'vitest';

import { trapTab } from './focusTrap';

describe('trapTab', () => {
  // Regression (D2): Tab walked out of every dialog into the dimmed app behind.
  it('wraps from the last element to the first, and back with Shift', () => {
    expect(trapTab(3, 2, false)).toBe(0);
    expect(trapTab(3, 0, true)).toBe(2);
  });

  it('leaves the moves between the edges to the browser', () => {
    expect(trapTab(3, 0, false)).toBeNull();
    expect(trapTab(3, 1, true)).toBeNull();
  });

  it('brings focus in when it is on the dialog itself or outside it', () => {
    expect(trapTab(3, -1, false)).toBe(0);
    expect(trapTab(3, -1, true)).toBe(2);
  });

  it('does nothing in a dialog with nothing to focus', () => {
    expect(trapTab(0, -1, false)).toBeNull();
  });
});
