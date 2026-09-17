// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { CloseIcon } from './icons';

/** A themed confirmation modal for destructive actions (hide / remove / delete).
 *  Enter confirms, Esc cancels. */
export function ConfirmDialog({
  title,
  message,
  confirmLabel,
  danger = true,
  onConfirm,
  onClose,
}: {
  title: string;
  message: React.ReactNode;
  confirmLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
      else if (e.key === 'Enter') {
        onConfirm();
        onClose();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onConfirm, onClose]);

  return (
    <div
      className="fixed inset-0 z-[80] grid place-items-center bg-void/70 p-4 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="w-full max-w-sm border border-line bg-surface p-6 shadow-card"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-3 flex items-start justify-between gap-3">
          <h2 className="font-display text-lg font-semibold text-ink">{title}</h2>
          <button
            onClick={onClose}
            className="grid h-8 w-8 shrink-0 place-items-center text-muted hover:bg-elevated hover:text-ink"
          >
            <CloseIcon className="h-5 w-5" />
          </button>
        </div>

        <p className="mb-6 text-sm leading-relaxed text-muted">{message}</p>

        <div className="flex justify-end gap-2">
          <button
            onClick={onClose}
            className="px-4 py-2 text-sm text-muted hover:text-ink"
          >
            {t('common.cancel')}
          </button>
          <button
            autoFocus
            onClick={() => {
              onConfirm();
              onClose();
            }}
            className={`px-4 py-2 text-sm font-semibold ${
              danger
                ? 'bg-destructive text-destructive-foreground hover:bg-destructive/90'
                : 'bg-accent text-white hover:bg-accent-soft'
            }`}
          >
            {confirmLabel ?? t('common.confirm')}
          </button>
        </div>
      </div>
    </div>
  );
}
