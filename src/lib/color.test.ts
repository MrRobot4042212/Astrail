// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { describe, expect, it } from 'vitest';
import { colorToCommit } from './color';

describe('colorToCommit', () => {
  it('commits a changed color', () => {
    expect(colorToCommit('#ff0000', '#00ff00')).toBe('#ff0000');
  });

  it('skips a commit when the picker closes on the stored color', () => {
    // Blur and the native change event both fire on close; the second is a no-op.
    expect(colorToCommit('#ff0000', '#ff0000')).toBeNull();
    expect(colorToCommit('#aabbcc', '#AABBCC')).toBeNull();
  });
});
