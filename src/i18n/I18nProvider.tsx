// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useEffect } from 'react';
import { I18nextProvider } from 'react-i18next';
import { useAppSettings } from '@/hooks/useAppSettings';
import { applyLanguage } from './applyLanguage';
import i18n, { resolveLanguage } from './config';

/**
 * Applies the saved UI language to i18next and keeps it in sync, through the
 * window's settings store, so changing the language in Ajustes (or from another
 * window) updates the whole app live.
 *
 * It starts from the OS language rather than blocking on the settings IPC: this
 * component wraps the entire app, and returning `null` until the round trip
 * finished delayed the first paint of every window by a full IPC call. The saved
 * preference is applied as soon as it arrives, which is a language switch, not a
 * flash of English (the OS default is already the right answer for most users).
 */
export function I18nProvider({ children }: { children: React.ReactNode }) {
  useEffect(() => {
    // `<html lang>` is static in the exported layout; keep it on the active
    // language so screen readers and hyphenation use the right one.
    const syncLang = (lng: string) => {
      document.documentElement.lang = lng;
    };
    i18n.on('languageChanged', syncLang);
    // The switch below is skipped when the language is already active, so the
    // listener alone would leave the exported layout's `lang` in place.
    syncLang(i18n.language);

    // Immediate best guess; `SavedLanguage` applies the saved preference when it
    // arrives (and keeps the OS language if the settings cannot be read).
    applyLanguage(i18n, resolveLanguage('system'));

    return () => {
      i18n.off('languageChanged', syncLang);
    };
  }, []);

  return (
    <I18nextProvider i18n={i18n}>
      <SavedLanguage />
      {children}
    </I18nextProvider>
  );
}

/**
 * Follows the saved language. A component of its own, rendering nothing, so a
 * settings change re-renders only this and never the tree under the provider;
 * and it switches only when the language itself changes, not on every other
 * setting (see `applyLanguage`).
 */
function SavedLanguage() {
  const language = useAppSettings().settings?.language;
  useEffect(() => {
    if (language !== undefined) applyLanguage(i18n, resolveLanguage(language));
  }, [language]);
  return null;
}
