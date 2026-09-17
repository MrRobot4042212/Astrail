// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getHiddenLibrary, unhideGame } from '@/lib/tauri';
import type { Game } from '@/lib/types';
import { CloseIcon, EyeOffIcon } from './icons';

export function HiddenGamesModal({
  onClose,
  onChanged,
}: {
  onClose: () => void;
  onChanged: () => void;
}) {
  const { t } = useTranslation();
  const [games, setGames] = useState<Game[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getHiddenLibrary()
      .then(setGames)
      .catch(console.error);
  }, []);

  async function handleUnhide(id: string) {
    setBusy(true);
    try {
      await unhideGame(id);
      setGames((prev) => prev.filter((g) => g.id !== id));
      onChanged();
    } catch (e) {
      console.error('Error al restaurar el elemento', e);
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
        className="flex w-full max-w-md max-h-[80vh] flex-col rounded-xl2 border border-line bg-surface p-6 shadow-card"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-5 flex shrink-0 items-center justify-between">
          <div className="flex items-center gap-2 text-ink">
            <EyeOffIcon className="h-5 w-5" />
            <h2 className="font-display text-lg font-semibold">{t('sidebar.hiddenItems')}</h2>
          </div>
          <button
            onClick={onClose}
            className="grid h-8 w-8 place-items-center rounded-lg text-muted hover:bg-elevated hover:text-ink"
          >
            <CloseIcon className="h-5 w-5" />
          </button>
        </div>

        <div className="flex-1 overflow-y-auto pr-2 custom-scrollbar">
          {games.length === 0 ? (
            <p className="text-sm leading-relaxed text-muted">{t('dialog.noHidden')}</p>
          ) : (
            <ul className="flex flex-col gap-2">
              {games.map((game) => (
                <li key={game.id} className="flex items-center justify-between border border-line bg-elevated/50 p-3 hover:bg-elevated transition-colors">
                  <div className="flex min-w-0 flex-col gap-0.5 pr-4">
                    <span className="truncate text-sm font-medium text-ink" title={game.name}>{game.name}</span>
                    <span className="text-[10px] uppercase tracking-wider text-muted">{game.source}</span>
                  </div>
                  <button
                    onClick={() => handleUnhide(game.id)}
                    disabled={busy}
                    className="shrink-0 border border-line bg-surface px-3 py-1.5 text-[11px] font-medium text-ink transition-colors hover:border-accent hover:text-accent disabled:opacity-50"
                  >
                    {t('dialog.restore')}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}
