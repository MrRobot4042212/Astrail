// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

/** What can take keyboard focus inside a dialog (visibility is checked apart). */
export const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), ' +
  'select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * Where Tab should move focus to keep it inside a dialog, or `null` to let the
 * browser move it. `current` is the index of the focused element among the
 * dialog's `count` focusable ones (-1 when focus is on the dialog itself or
 * outside it). Only the two edges wrap; anything between is the browser's.
 */
export function trapTab(count: number, current: number, shift: boolean): number | null {
  if (count === 0) return null;
  if (current === -1) return shift ? count - 1 : 0;
  if (shift && current === 0) return count - 1;
  if (!shift && current === count - 1) return 0;
  return null;
}
