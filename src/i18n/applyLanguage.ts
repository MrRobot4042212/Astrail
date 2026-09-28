// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import type { i18n as I18n } from 'i18next';

/** The part of an i18next instance a language switch needs. */
export type LanguageSwitch = Pick<I18n, 'language' | 'changeLanguage'>;

/**
 * Make `lng` the active language, doing nothing when it already is.
 *
 * i18next emits `languageChanged` on every `changeLanguage` call, even for the
 * language already active, and every `useTranslation` consumer re-renders on it:
 * each `GameCard` of the library. The language was re-applied on every
 * `settings-updated`, i.e. on every settings change and every press of the overlay
 * hotkey, so the whole grid re-rendered mid-game for nothing (2026-09-27 audit,
 * X-G1).
 */
export function applyLanguage(i18n: LanguageSwitch, lng: string): void {
  if (i18n.language === lng) return;
  void i18n.changeLanguage(lng);
}
