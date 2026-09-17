// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-dialog';
import { addManualApp } from '@/lib/tauri';
import type { Game } from '@/lib/types';
import { CloseIcon, FolderIcon } from './icons';

function nameFromPath(p: string): string {
  const file = p.split(/[\\/]/).pop() ?? p;
  return file.replace(/\.(exe|lnk|bat)$/i, '');
}

export function AddAppDialog({
  onClose,
  onAdded,
}: {
  onClose: () => void;
  onAdded: (g: Game) => void;
}) {
  const { t } = useTranslation();
  const [name, setName] = useState('');
  const [exe, setExe] = useState('');
  const [cover, setCover] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function pickExe() {
    setError(null);
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: t('dialog.appsFilter'), extensions: ['exe', 'lnk', 'bat'] }],
    });
    if (typeof selected === 'string') {
      setExe(selected);
      if (!name) setName(nameFromPath(selected));
    }
  }

  async function submit() {
    if (!name.trim() || !exe.trim()) {
      setError(t('dialog.addNeedExeName'));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const game = await addManualApp(name.trim(), exe.trim(), cover.trim() || undefined);
      onAdded(game);
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
        className="w-full max-w-md rounded-xl2 border border-line bg-surface p-6 shadow-card"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-5 flex items-center justify-between">
          <h2 className="font-display text-lg font-semibold text-ink">
            {t('dialog.addTitle')}
          </h2>
          <button
            onClick={onClose}
            className="grid h-8 w-8 place-items-center rounded-lg text-muted hover:bg-elevated hover:text-ink"
          >
            <CloseIcon className="h-5 w-5" />
          </button>
        </div>

        <label className="mb-1.5 block text-xs font-medium text-muted">
          {t('dialog.executable')}
        </label>
        <button
          onClick={pickExe}
          className="mb-4 flex w-full items-center gap-2 truncate rounded-lg border border-line bg-elevated px-3 py-2.5 text-left text-sm text-ink hover:border-accent/40"
        >
          <FolderIcon className="h-[18px] w-[18px] shrink-0 text-accent" />
          <span className="truncate">{exe || t('dialog.selectFile')}</span>
        </button>

        <label className="mb-1.5 block text-xs font-medium text-muted">
          {t('dialog.name')}
        </label>
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder={t('dialog.namePlaceholder')}
          className="mb-4 w-full rounded-lg border border-line bg-elevated px-3 py-2.5 text-sm text-ink outline-none placeholder:text-muted/60 focus:border-accent/60"
        />

        <label className="mb-1.5 block text-xs font-medium text-muted">
          {t('dialog.coverUrl')} <span className="text-muted/60">{t('dialog.optional')}</span>
        </label>
        <input
          value={cover}
          onChange={(e) => setCover(e.target.value)}
          placeholder="https://…/cover.jpg"
          className="mb-5 w-full rounded-lg border border-line bg-elevated px-3 py-2.5 text-sm text-ink outline-none placeholder:text-muted/60 focus:border-accent/60"
        />

        {error && <p className="mb-4 text-sm text-accent">{error}</p>}

        <div className="flex justify-end gap-2">
          <button
            onClick={onClose}
            className="rounded-lg px-4 py-2 text-sm text-muted hover:text-ink"
          >
            {t('common.cancel')}
          </button>
          <button
            onClick={submit}
            disabled={busy}
            className="rounded-lg bg-accent px-4 py-2 text-sm font-semibold text-white hover:bg-accent-soft disabled:opacity-50"
          >
            {busy ? t('dialog.adding') : t('common.add')}
          </button>
        </div>
      </div>
    </div>
  );
}
