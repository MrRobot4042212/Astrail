// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { useEffect, useRef } from 'react';

/**
 * When the detail page closes, put keyboard focus back on the card (or poster)
 * that opened it. The library remounts behind the detail page, so focus used to
 * fall to the page body and a keyboard user started over from the top bar
 * (2026-09-27 audit, D1). Cards carry `data-game-id`.
 */
export function useReturnFocusToCard(openId: string | null): void {
  const last = useRef<string | null>(null);
  useEffect(() => {
    if (openId) {
      last.current = openId;
      return;
    }
    const id = last.current;
    last.current = null;
    if (!id) return;
    // Effects run after the commit that mounted the library again, so the card
    // is already in the DOM.
    document.querySelector<HTMLElement>(`[data-game-id="${CSS.escape(id)}"]`)?.focus();
  }, [openId]);
}
