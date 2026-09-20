// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { recordShortcut } from '@/lib/shortcuts';

/** A button that captures a keyboard shortcut combination. */
export function ShortcutInput({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const { t } = useTranslation();
  const [recording, setRecording] = useState(false);
  const [needsModifier, setNeedsModifier] = useState(false);

  useEffect(() => {
    if (!recording) return;

    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      const result = recordShortcut(e);
      if (result.kind === 'cancel') {
        setRecording(false);
        setNeedsModifier(false);
      } else if (result.kind === 'needs-modifier') {
        setNeedsModifier(true);
      } else if (result.kind === 'combo') {
        onChange(result.value);
        setRecording(false);
        setNeedsModifier(false);
      }
    };

    window.addEventListener('keydown', onKeyDown, { capture: true });
    return () => window.removeEventListener('keydown', onKeyDown, { capture: true });
  }, [recording, onChange]);

  return (
    <button
      onClick={() => setRecording(true)}
      className={`w-full border px-3 py-2 text-sm text-left transition ${
        recording ? 'border-accent bg-accent/10 text-ink' : 'border-line bg-elevated text-muted hover:text-ink'
      }`}
    >
      {recording ? t(needsModifier ? 'settings.mShortcutNeedsModifier' : 'settings.mRecording') : value}
    </button>
  );
}
