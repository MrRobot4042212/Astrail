// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useEscape } from '@/hooks/useEscape';

export type MenuItem =
  | { type?: 'item'; label: string; icon?: React.ReactNode; onClick: () => void; danger?: boolean }
  | { type: 'separator' };

/** A floating right-click menu anchored at (x, y). Closes on outside click, Esc,
 *  scroll or resize. Clamps itself to stay inside the viewport.
 *
 *  Also a keyboard menu (2026-09-27 audit, D1): it opens from the Menu key or
 *  Shift+F10 on a card, so focus moves to its first item, ↑/↓/Home/End move
 *  between items, Enter picks one, Tab or Esc closes it, and focus goes back to
 *  where it was. */
export function ContextMenu({
  x,
  y,
  items,
  onClose,
}: {
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });

  // Keep the menu on screen after it knows its size.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const pad = 8;
    setPos({
      x: Math.min(x, window.innerWidth - r.width - pad),
      y: Math.min(y, window.innerHeight - r.height - pad),
    });
  }, [x, y]);

  useEscape(onClose);

  const itemsOf = () =>
    Array.from(ref.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? []);

  useEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    ref.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus({ preventScroll: true });
    return () => {
      if (previous?.isConnected) previous.focus({ preventScroll: true });
    };
  }, []);

  function onKeyDown(e: React.KeyboardEvent<HTMLDivElement>) {
    if (e.key === 'Tab') {
      e.preventDefault();
      onClose();
      return;
    }
    const all = itemsOf();
    if (all.length === 0) return;
    const at = all.indexOf(document.activeElement as HTMLElement);
    const next =
      e.key === 'ArrowDown' ? (at + 1) % all.length
      : e.key === 'ArrowUp' ? (at <= 0 ? all.length - 1 : at - 1)
      : e.key === 'Home' ? 0
      : e.key === 'End' ? all.length - 1
      : null;
    if (next === null) return;
    e.preventDefault();
    all[next].focus();
  }

  useEffect(() => {
    window.addEventListener('resize', onClose);
    window.addEventListener('scroll', onClose, true);
    return () => {
      window.removeEventListener('resize', onClose);
      window.removeEventListener('scroll', onClose, true);
    };
  }, [onClose]);

  return (
    <div className="fixed inset-0 z-[60]" onClick={onClose} onContextMenu={(e) => e.preventDefault()}>
      <div
        ref={ref}
        role="menu"
        data-tour="context-menu"
        style={{ left: pos.x, top: pos.y }}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={onKeyDown}
        className="absolute min-w-[200px] border border-line bg-popover py-1 shadow-card"
      >
        {items.map((it, i) =>
          'type' in it && it.type === 'separator' ? (
            <div key={i} role="separator" className="my-1 h-px bg-line" />
          ) : (
            <button
              key={i}
              role="menuitem"
              onClick={() => {
                (it as Extract<MenuItem, { onClick: () => void }>).onClick();
                onClose();
              }}
              className={`flex w-full items-center gap-3 px-3 py-2 text-left text-sm transition-colors hover:bg-elevated focus-visible:bg-elevated focus-visible:outline-none ${
                (it as { danger?: boolean }).danger
                  ? 'text-destructive hover:text-destructive'
                  : 'text-ink'
              }`}
            >
              <span className="grid h-4 w-4 place-items-center text-muted">
                {(it as { icon?: React.ReactNode }).icon}
              </span>
              <span className="flex-1">{(it as { label: string }).label}</span>
            </button>
          ),
        )}
      </div>
    </div>
  );
}
