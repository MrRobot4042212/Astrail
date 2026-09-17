// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { renameCategory, setCategoryIcon } from '@/lib/tauri';
import { CATEGORY_ICONS, CATEGORY_ICON_KEYS } from '@/lib/categoryIcons';
import type { Category } from '@/lib/types';
import { CloseIcon, TagIcon } from './icons';

/** Edit a category: rename it and/or change its icon. Renaming to an existing
 *  name merges the two. */
export function EditCategoryDialog({
  category,
  existing,
  onClose,
  onSaved,
}: {
  category: Category;
  /** Other category names, to warn about merges. */
  existing: string[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const [name, setName] = useState(category.name);
  const [icon, setIcon] = useState<string | null>(category.icon ?? null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const merges = existing.some(
    (c) => c.toLowerCase() === name.trim().toLowerCase() && c.toLowerCase() !== category.name.toLowerCase(),
  );

  async function commit() {
    const value = name.trim();
    if (!value) return;
    setBusy(true);
    setError(null);
    try {
      if (value.toLowerCase() !== category.name.toLowerCase()) {
        await renameCategory(category.name, value);
      }
      // Apply the icon to the (possibly new) name.
      if ((icon ?? null) !== (category.icon ?? null) || value !== category.name) {
        await setCategoryIcon(value, icon);
      }
      onSaved();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-void/70 p-4 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="w-full max-w-sm border border-line bg-surface p-6 shadow-card"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-5 flex items-center justify-between">
          <h2 className="font-display text-lg font-semibold text-ink">{t('dialog.editCategory')}</h2>
          <button
            onClick={onClose}
            className="grid h-8 w-8 shrink-0 place-items-center text-muted hover:bg-elevated hover:text-ink"
          >
            <CloseIcon className="h-5 w-5" />
          </button>
        </div>

        <label className="mb-1.5 block text-xs font-medium text-muted">{t('dialog.name')}</label>
        <input
          autoFocus
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault();
              commit();
            }
          }}
          className="w-full border border-line bg-elevated px-3 py-2.5 text-sm text-ink outline-none placeholder:text-muted/60 focus:border-accent/60"
        />
        {merges && (
          <p className="mt-2 text-xs text-info">{t('dialog.mergeWarn', { name: name.trim() })}</p>
        )}

        <label className="mb-1.5 mt-4 block text-xs font-medium text-muted">{t('dialog.icon')}</label>
        <div className="grid grid-cols-8 gap-1.5">
          <button
            onClick={() => setIcon(null)}
            title={t('dialog.noIcon')}
            className={`grid aspect-square place-items-center border transition ${
              icon === null
                ? 'border-accent bg-accent/15 text-ink'
                : 'border-line text-muted hover:border-accent/50 hover:text-ink'
            }`}
          >
            <TagIcon className="h-[18px] w-[18px]" />
          </button>
          {CATEGORY_ICON_KEYS.map((key) => {
            const Icon = CATEGORY_ICONS[key];
            const active = icon === key;
            return (
              <button
                key={key}
                onClick={() => setIcon(key)}
                title={key}
                className={`grid aspect-square place-items-center border transition ${
                  active
                    ? 'border-accent bg-accent/15 text-ink'
                    : 'border-line text-muted hover:border-accent/50 hover:text-ink'
                }`}
              >
                <Icon className="h-[18px] w-[18px]" />
              </button>
            );
          })}
        </div>

        {error && <p className="mt-4 text-sm text-accent">{error}</p>}

        <div className="mt-5 flex justify-end gap-2">
          <button onClick={onClose} className="px-4 py-2 text-sm text-muted hover:text-ink">
            {t('common.cancel')}
          </button>
          <button
            onClick={commit}
            disabled={busy || !name.trim()}
            className="bg-accent px-4 py-2 text-sm font-semibold text-white hover:bg-accent-soft disabled:opacity-50"
          >
            {busy ? t('dialog.saving') : t('common.save')}
          </button>
        </div>
      </div>
    </div>
  );
}
