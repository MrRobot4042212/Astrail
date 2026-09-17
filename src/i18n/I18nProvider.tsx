'use client';

import { useEffect } from 'react';
import { I18nextProvider } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { getAppSettings } from '@/lib/tauri';
import i18n, { resolveLanguage } from './config';

/**
 * Applies the saved UI language to i18next and keeps it in sync. Re-applies on
 * the `settings-updated` event so changing the language in Ajustes (or from
 * another window) updates the whole app live.
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

    // Immediate best guess, then the saved preference when it arrives.
    i18n.changeLanguage(resolveLanguage('system'));

    const apply = () =>
      getAppSettings()
        .then((s) => i18n.changeLanguage(resolveLanguage(s.language)))
        .catch(() => {
          // Keep the OS-derived language.
        });

    apply();
    const un = listen('settings-updated', apply);
    un.catch(() => {});

    return () => {
      i18n.off('languageChanged', syncLang);
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  return <I18nextProvider i18n={i18n}>{children}</I18nextProvider>;
}
