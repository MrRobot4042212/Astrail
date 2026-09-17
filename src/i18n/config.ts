// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import { en } from './en';
import { es } from './es';

export type Lang = 'es' | 'en';

/**
 * Resolve the effective UI language from the stored setting:
 * "es" / "en" are explicit; anything else ("system" or unset) follows the OS /
 * webview locale, falling back to English.
 */
export function resolveLanguage(setting: string | undefined | null): Lang {
  if (setting === 'es' || setting === 'en') return setting;
  const sys = (typeof navigator !== 'undefined' ? navigator.language : 'en') || 'en';
  return sys.toLowerCase().startsWith('es') ? 'es' : 'en';
}

if (!i18n.isInitialized) {
  i18n.use(initReactI18next).init({
    resources: {
      en: { translation: en },
      es: { translation: es },
    },
    lng: 'en',
    fallbackLng: 'en',
    interpolation: { escapeValue: false }, // React already escapes
    returnNull: false,
  });
}

export default i18n;
