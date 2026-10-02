// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

/** The privacy policy on the project's site, in the language of the UI. The
 *  site serves English without a prefix and Spanish under `/es`. Opened through
 *  `openExternal`, whose allowlist in the core includes this host. */
export function privacyUrl(language: string): string {
  return language.toLowerCase().startsWith('es') ? 'https://astrail.es/es/privacy' : 'https://astrail.es/privacy';
}
