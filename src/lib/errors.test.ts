// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { describe, expect, it } from 'vitest';
import { errorMessageKey, isAppError } from './errors';
import { en } from '../i18n/en';
import { es } from '../i18n/es';

describe('command errors', () => {
  it('translates the code a command rejected with', () => {
    expect(errorMessageKey({ code: 'autostart_unavailable', detail: 'not installed' })).toEqual({
      key: 'errors.code.autostart_unavailable',
      detail: 'not installed',
    });
  });

  it('never prints [object Object] for something that is not an AppError', () => {
    // Regression: every catch site did `String(e)` on the rejection.
    expect(errorMessageKey(new Error('boom'))).toEqual({ key: 'errors.code.internal', detail: 'boom' });
    expect(errorMessageKey('plain text')).toEqual({ key: 'errors.code.internal', detail: 'plain text' });
    expect(errorMessageKey({ code: 'made_up', detail: 'x' }).key).toBe('errors.code.internal');
    expect(isAppError({ code: 'io' })).toBe(false);
    expect(isAppError(null)).toBe(false);
  });

  it('has a translation for every code in both catalogs', () => {
    for (const catalog of [es, en]) {
      const codes = catalog.errors.code;
      for (const code of Object.keys(codes)) {
        expect(isAppError({ code, detail: '' }), code).toBe(true);
      }
      expect(Object.keys(codes)).toHaveLength(8);
    }
  });
});
