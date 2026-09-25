// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { useEffect, useRef } from 'react';
import { pushEscape } from '@/lib/escapeStack';

/**
 * Close this layer on Escape, but only while it is the top-most one (see
 * `escapeStack`). The layer is registered while `active` is true; a new
 * `onEscape` identity does not re-register it, so it keeps its place in the
 * stack instead of jumping above layers opened after it.
 */
export function useEscape(onEscape: () => void, active = true): void {
  const latest = useRef(onEscape);
  useEffect(() => {
    latest.current = onEscape;
  });
  useEffect(() => {
    if (!active) return;
    return pushEscape(() => latest.current());
  }, [active]);
}
