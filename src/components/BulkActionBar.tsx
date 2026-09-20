// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';

import { EyeOffIcon, StarIcon, TagIcon } from './icons';

/** Floating bar shown while entries are selected in the grid. */
export function BulkActionBar({
  count,
  onSelectAll,
  onFavorite,
  onCategories,
  onHide,
  onCancel,
}: {
  count: number;
  onSelectAll: () => void;
  onFavorite: () => void;
  onCategories: () => void;
  onHide: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="fixed bottom-14 left-1/2 z-50 flex -translate-x-1/2 items-center gap-2 border border-line bg-elevated px-3 py-2 shadow-card">
      <span className="px-2 text-sm font-medium text-ink">{t('bulk.selected', { count })}</span>
      <BarButton onClick={onSelectAll}>{t('bulk.selectAll')}</BarButton>
      <div className="mx-1 h-6 w-px bg-line" />
      <BarButton onClick={onFavorite}>
        <StarIcon className="h-4 w-4" /> {t('bulk.favorite')}
      </BarButton>
      <BarButton onClick={onCategories}>
        <TagIcon className="h-4 w-4" /> {t('bulk.categories')}
      </BarButton>
      <BarButton onClick={onHide} danger>
        <EyeOffIcon className="h-4 w-4" /> {t('bulk.hide')}
      </BarButton>
      <div className="mx-1 h-6 w-px bg-line" />
      <BarButton onClick={onCancel}>{t('bulk.cancel')}</BarButton>
    </div>
  );
}

function BarButton({
  children,
  onClick,
  danger,
}: {
  children: ReactNode;
  onClick: () => void;
  danger?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      className={`flex items-center gap-1.5 px-3 py-1.5 text-sm text-muted transition hover:bg-surface ${
        danger ? 'hover:text-destructive' : 'hover:text-ink'
      }`}
    >
      {children}
    </button>
  );
}
