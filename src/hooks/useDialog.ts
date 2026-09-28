// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { useCallback, useEffect, useRef, type KeyboardEvent } from 'react';
import { useEscape } from '@/hooks/useEscape';
import { FOCUSABLE, trapTab } from '@/lib/focusTrap';

function focusables(panel: HTMLElement): HTMLElement[] {
  return Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (el) => el.getClientRects().length > 0,
  );
}

/**
 * Make a panel a modal dialog. Spread the result on the panel (not the backdrop).
 *
 * No dialog declared itself one: nothing told a screen reader a modal had
 * opened, Tab walked out into the dimmed app behind it, focus was lost on close,
 * and eight of them never joined the Escape stack, so Escape closed the layer
 * underneath instead (2026-09-27 audit, D2 / G4). This gives every dialog:
 * - `role="dialog"`, `aria-modal`, and its title as the accessible name;
 * - focus moved inside on open (an `autoFocus` element keeps it), kept inside
 *   while Tab moves, and returned to where it was on close;
 * - Escape through `useEscape`, so only the top-most layer closes.
 */
export function useDialog<T extends HTMLElement = HTMLDivElement>({
  onClose,
  labelledBy,
  active = true,
}: {
  onClose: () => void;
  /** Id of the element whose text names the dialog, usually its title. */
  labelledBy?: string;
  active?: boolean;
}) {
  const ref = useRef<T>(null);
  useEscape(onClose, active);

  useEffect(() => {
    if (!active) return;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const panel = ref.current;
    if (panel && !panel.contains(document.activeElement)) {
      // A form's first field rather than the close button in the corner.
      const items = focusables(panel);
      const field = items.find((el) => el.matches('input, select, textarea'));
      (field ?? items[0] ?? panel).focus({ preventScroll: true });
    }
    return () => {
      if (previous?.isConnected) previous.focus({ preventScroll: true });
    };
  }, [active]);

  const onKeyDown = useCallback((e: KeyboardEvent<T>) => {
    if (e.key !== 'Tab' || !ref.current) return;
    const items = focusables(ref.current);
    const current = items.indexOf(document.activeElement as HTMLElement);
    const next = trapTab(items.length, current, e.shiftKey);
    if (next === null) return;
    e.preventDefault();
    items[next].focus();
  }, []);

  return {
    ref,
    role: 'dialog' as const,
    'aria-modal': true as const,
    'aria-labelledby': labelledBy,
    tabIndex: -1,
    onKeyDown,
  };
}
