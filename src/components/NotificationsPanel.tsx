// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CloseIcon, BookIcon } from './icons';
import { getAppSettings, patchAppSettings } from '@/lib/tauri';
import { DEFAULT_SHORTCUTS, formatShortcut } from '@/lib/shortcuts';
interface Tutorial {
  title: string;
  description: string;
}

export function NotificationsPanel({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Reflect the user's real Spotlight binding (custom or our default).
  const [spotlight, setSpotlight] = useState(DEFAULT_SHORTCUTS.spotlight);
  useEffect(() => {
    getAppSettings()
      .then((s) => s.shortcuts?.spotlight && setSpotlight(s.shortcuts.spotlight))
      .catch(() => {});
  }, []);

  const tutorials: Tutorial[] = [
    { title: t('notifications.welcomeTitle'), description: t('notifications.welcomeBody') },
    { title: t('notifications.spotlightTitle'), description: t('notifications.spotlightBody', { shortcut: formatShortcut(spotlight, t('common.keySpace')).join('+') }) },
    { title: t('notifications.coversTitle'), description: t('notifications.coversBody') },
    { title: t('notifications.manualTitle'), description: t('notifications.manualBody') },
  ];

  async function resetOnboarding() {
    setBusy(true);
    setError(null);
    try {
      await patchAppSettings({ setup_completed: false });
      window.location.reload();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  }

  return (
    <>
      <div
        className="fixed inset-0 z-40 bg-void/40 backdrop-blur-sm"
        onClick={onClose}
      />
      <div className="fixed right-0 top-0 bottom-0 z-50 w-80 border-l border-line bg-surface shadow-2xl flex flex-col">
        <div className="flex items-center justify-between border-b border-line px-5 py-4">
          <div className="flex items-center gap-2">
            <BookIcon className="h-5 w-5 text-accent" />
            <h2 className="font-display text-base font-semibold text-ink">{t('notifications.title')}</h2>
          </div>
          <button
            onClick={onClose}
            className="grid h-8 w-8 place-items-center rounded-lg text-muted hover:bg-elevated hover:text-ink"
          >
            <CloseIcon className="h-5 w-5" />
          </button>
        </div>

        <div className="flex-1 overflow-y-auto p-5 flex flex-col gap-5">
          {tutorials.map((tut, i) => (
            <div key={i} className="relative rounded-xl border border-line bg-elevated p-4">
              <div className="mb-1.5 flex items-center gap-2">
                <span className="flex h-5 w-5 items-center justify-center rounded-full bg-accent/20 text-[11px] font-bold text-accent">
                  {i + 1}
                </span>
                <h3 className="text-sm font-semibold text-ink">{tut.title}</h3>
              </div>
              <p className="text-xs leading-relaxed text-muted pl-7">
                {tut.description}
              </p>
            </div>
          ))}

          <div className="mt-4 rounded-lg bg-accent/10 p-4 border border-accent/20">
            <p className="text-xs text-accent">
              {t('notifications.hintHidden')}
            </p>
          </div>

          <div className="mt-4 border-t border-line pt-5">
            <button
              onClick={resetOnboarding}
              disabled={busy}
              className="w-full rounded-lg border border-line bg-elevated px-4 py-2.5 text-sm font-medium text-ink transition hover:border-accent/40 disabled:opacity-50"
            >
              {t('notifications.redoOnboarding')}
            </button>
            {error && <p className="mt-2 text-center text-xs text-accent">{error}</p>}
          </div>
        </div>
      </div>
    </>
  );
}
