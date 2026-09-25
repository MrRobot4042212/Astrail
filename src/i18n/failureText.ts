// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import i18n from './config';
import { errorText } from '@/lib/errors';

/**
 * The text to show for a failed command, in the current language.
 *
 * Uses the app's i18n instance rather than a `t` from `useTranslation`, so a
 * `.catch` inside an effect or a memoized callback does not have to list `t`
 * as a dependency (which would re-run it on every language change).
 */
export function failureText(e: unknown): string {
  return errorText(e, i18n.t.bind(i18n));
}
