// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { describe, expect, it } from 'vitest';

import { privacyUrl } from './privacy';

describe('privacyUrl', () => {
  it('links the policy in the language of the UI', () => {
    expect(privacyUrl('es')).toBe('https://astrail.es/es/privacy');
    expect(privacyUrl('es-ES')).toBe('https://astrail.es/es/privacy');
    expect(privacyUrl('en')).toBe('https://astrail.es/privacy');
    // Anything the site has no translation for gets the English page.
    expect(privacyUrl('fr')).toBe('https://astrail.es/privacy');
  });
});
