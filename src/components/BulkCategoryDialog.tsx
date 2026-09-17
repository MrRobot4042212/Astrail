// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CloseIcon, PlusIcon } from './icons';

/** Pick one or more categories to **add** to several games at once. Only adds
 *  (never removes), so mixed selections aren't clobbered. */
export function BulkCategoryDialog({
  count,
  allCategories,
  onClose,
  onApply,
}: {
  /** How many games the categories will be applied to. */
  count: number;
  allCategories: string[];
  onClose: () => void;
  onApply: (categories: string[]) => Promise<void> | void;
}) {
  const { t } = useTranslation();
  const [chosen, setChosen] = useState<string[]>([]);
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);

  const options = Array.from(new Set([...allCategories, ...chosen])).sort((a, b) =>
    a.localeCompare(b),
  );
  const isActive = (name: string) => chosen.some((c) => c.toLowerCase() === name.toLowerCase());

  function toggle(name: string) {
    setChosen((prev) =>
      isActive(name)
        ? prev.filter((c) => c.toLowerCase() !== name.toLowerCase())
        : [...prev, name],
    );
  }

  function addDraft() {
    const name = draft.trim();
    if (name && !isActive(name)) setChosen((prev) => [...prev, name]);
    setDraft('');
  }

  async function commit() {
    if (chosen.length === 0) return;
    setBusy(true);
    try {
      await onApply(chosen);
      onClose();
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      className="fixed inset-0 z-[55] grid place-items-center bg-void/70 p-4 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="w-full max-w-md rounded-xl2 border border-line bg-surface p-6 shadow-card"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-5 flex items-center justify-between">
          <h2 className="font-display text-lg font-semibold text-ink">
            {t('dialog.bulkTitle', { count })}
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

        <label className="mb-1.5 block text-xs font-medium text-muted">{t('sidebar.newCategory')}</label>
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

        <div className="mt-5 flex justify-end gap-2">
          <button onClick={onClose} className="rounded-lg px-4 py-2 text-sm text-muted hover:text-ink">
            {t('common.cancel')}
          </button>
          <button
            onClick={commit}
            disabled={busy || chosen.length === 0}
            className="rounded-lg bg-accent px-4 py-2 text-sm font-semibold text-white hover:bg-accent-soft disabled:opacity-50"
          >
            {busy ? t('dialog.applying') : t('dialog.apply')}
          </button>
        </div>
      </div>
    </div>
  );
}
