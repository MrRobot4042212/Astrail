// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { createInstance } from 'i18next';
import { describe, expect, it } from 'vitest';

import { applyLanguage } from './applyLanguage';

async function instance() {
  const i18n = createInstance();
  await i18n.init({
    resources: { en: { translation: {} }, es: { translation: {} } },
    lng: 'en',
    fallbackLng: 'en',
  });
  return i18n;
}

describe('applyLanguage', () => {
  it('does not announce a change when the language is already active', async () => {
    // Regression (X-G1): every settings change re-applied the same language, and
    // i18next announced it anyway, re-rendering every translated component.
    const i18n = await instance();
    let announced = 0;
    i18n.on('languageChanged', () => announced++);

    // The premise: i18next itself announces a switch to the active language.
    await i18n.changeLanguage('en');
    expect(announced).toBe(1);

    applyLanguage(i18n, 'en');
    applyLanguage(i18n, 'en');
    await Promise.resolve();
    expect(announced).toBe(1);
  });

  it('switches when the language differs', async () => {
    const i18n = await instance();
    const switched = new Promise<string>((resolve) => i18n.on('languageChanged', resolve));
    applyLanguage(i18n, 'es');
    await expect(switched).resolves.toBe('es');
    expect(i18n.language).toBe('es');
  });
});
