// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { setCategories } from '@/lib/tauri';
import type { Game } from '@/lib/types';
import { CloseIcon, PlusIcon } from './icons';

/** Assign manual categories to a single game. The user can toggle any existing
 *  category or type a new one; categories live only as long as a game uses them. */
export function CategoryDialog({
  game,
  allCategories,
  onClose,
  onSaved,
}: {
  game: Game;
  /** Every category currently in use across the library, for quick toggling. */
  allCategories: string[];
  onClose: () => void;
  onSaved: (id: string, categories: string[]) => void;
}) {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<string[]>(game.categories ?? []);
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Union of in-use categories and ones already on this game, shown as chips.
  const options = Array.from(new Set([...allCategories, ...selected])).sort((a, b) =>
    a.localeCompare(b),
  );

  function toggle(name: string) {
    setSelected((prev) =>
      prev.some((c) => c.toLowerCase() === name.toLowerCase())
        ? prev.filter((c) => c.toLowerCase() !== name.toLowerCase())
        : [...prev, name],
    );
  }

  function addDraft() {
    const name = draft.trim();
    if (!name) return;
    if (!selected.some((c) => c.toLowerCase() === name.toLowerCase())) {
      setSelected((prev) => [...prev, name]);
    }
    setDraft('');
  }

  async function commit() {
    setBusy(true);
    setError(null);
    try {
      await setCategories(game.id, selected);
      onSaved(game.id, selected);
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  const isActive = (name: string) =>
    selected.some((c) => c.toLowerCase() === name.toLowerCase());

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-void/70 p-4 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="w-full max-w-md rounded-xl2 border border-line bg-surface p-6 shadow-card"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-5 flex items-center justify-between">
          <h2 className="line-clamp-1 font-display text-lg font-semibold text-ink">
            {t('dialog.categoriesTitle', { name: game.name })}
          </h2>
          <button
            onClick={onClose}
            className="grid h-8 w-8 shrink-0 place-items-center rounded-lg text-muted hover:bg-elevated hover:text-ink"
          >
            <CloseIcon className="h-5 w-5" />
          </button>
        </div>

        {options.length > 0 && (
          <div className="mb-4 flex flex-wrap gap-2">
            {options.map((name) => (
              <button
                key={name}
                onClick={() => toggle(name)}
                className={`rounded-full border px-3 py-1.5 text-sm transition ${
                  isActive(name)
                    ? 'border-accent bg-accent/15 text-ink'
                    : 'border-line text-muted hover:border-accent/50 hover:text-ink'
                }`}
              >
                {name}
              </button>
            ))}
          </div>
        )}

        <label className="mb-1.5 block text-xs font-medium text-muted">
          {t('sidebar.newCategory')}
        </label>
        <div className="flex gap-2">
          <input
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                addDraft();
              }
            }}
            placeholder={t('dialog.categoryPlaceholder')}
            className="w-full rounded-lg border border-line bg-elevated px-3 py-2.5 text-sm text-ink outline-none placeholder:text-muted/60 focus:border-accent/60"
          />
          <button
            onClick={addDraft}
            title={t('dialog.addCategory')}
            className="grid h-[42px] w-[42px] shrink-0 place-items-center rounded-lg border border-line text-muted transition hover:text-accent"
          >
            <PlusIcon className="h-[18px] w-[18px]" />
          </button>
        </div>

        {error && <p className="mt-4 text-sm text-accent">{error}</p>}

        <div className="mt-5 flex justify-end gap-2">
          <button
            onClick={onClose}
            className="rounded-lg px-4 py-2 text-sm text-muted hover:text-ink"
          >
            {t('common.cancel')}
          </button>
          <button
            onClick={commit}
            disabled={busy}
            className="rounded-lg bg-accent px-4 py-2 text-sm font-semibold text-white hover:bg-accent-soft disabled:opacity-50"
          >
            {busy ? t('dialog.saving') : t('common.save')}
          </button>
        </div>
      </div>
    </div>
  );
}
