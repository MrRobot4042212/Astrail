// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useEffect, useRef, useState } from 'react';
import { colorToCommit } from '@/lib/color';

/** Color picker: a native <input type="color"> styled to look like the Astrail design. */
export function ColorPicker({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
}) {
  // While the picker is open, drag ticks only update this local draft; the
  // settings are written once, when the picker closes (native `change`) or loses
  // focus. React's `onChange` on a color input is the per-tick `input` event.
  const [draft, setDraft] = useState<string | null>(null);
  const shown = draft ?? value;
  const inputRef = useRef<HTMLInputElement>(null);
  const lastSent = useRef(value);
  const commitRef = useRef<(next: string) => void>(() => {});

  useEffect(() => {
    lastSent.current = value;
    commitRef.current = (next: string) => {
      setDraft(null);
      const commit = colorToCommit(next, lastSent.current);
      if (commit === null) return;
      lastSent.current = commit;
      onChange(commit);
    };
  }, [value, onChange]);

  useEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    const onNativeChange = () => commitRef.current(el.value);
    el.addEventListener('change', onNativeChange);
    return () => el.removeEventListener('change', onNativeChange);
  }, []);

  return (
    <div className="flex items-center justify-between gap-3">
      <span className="text-xs text-muted">{label}</span>
      <label
        className="relative h-7 w-10 cursor-pointer overflow-hidden border border-line transition hover:border-accent/60"
        title={shown}
      >
        {/* Color swatch visible surface */}
        <div className="absolute inset-0" style={{ background: shown }} />
        {/* Native color input sits on top, invisible but captures clicks */}
        <input
          ref={inputRef}
          type="color"
          value={shown}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={(e) => commitRef.current(e.currentTarget.value)}
          className="absolute inset-0 h-full w-full cursor-pointer opacity-0"
        />
      </label>
    </div>
  );
}
