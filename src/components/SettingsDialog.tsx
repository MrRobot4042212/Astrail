// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

// The settings drawer: tab rail, slide-in and close. State and actions live in
// `useSettingsModel`, each tab in `./settings/`.

import { useEffect, useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useSettingsModel } from '@/hooks/useSettingsModel';
import { CloseIcon, InfoIcon, GearIcon, FireIcon, BookIcon } from './icons';
import { AboutTab } from './AboutTab';
import { AppTab } from './settings/AppTab';
import { MetricsTab } from './settings/MetricsTab';
import { SystemTab } from './settings/SystemTab';
import { useDialog } from '@/hooks/useDialog';

type Tab = 'system' | 'app' | 'metrics' | 'about';

const TABS: { id: Tab; tKey: string; icon: typeof InfoIcon }[] = [
  { id: 'metrics', tKey: 'settings.tabMetrics', icon: FireIcon },
  { id: 'system', tKey: 'settings.tabSystem', icon: InfoIcon },
  { id: 'app', tKey: 'settings.tabApp', icon: GearIcon },
  { id: 'about', tKey: 'settings.tabAbout', icon: BookIcon },
];

export function SettingsDialog({
  onClose,
  onChanged,
  onStartTour,
}: {
  onClose: () => void;
  /** Called after a change (cache wiped / hidden restored) so the library can refresh. */
  onChanged: () => void;
  /** Re-launch the guided product tour. */
  onStartTour?: () => void;
}) {
  const model = useSettingsModel({ onChanged, onClose });
  // The first tab of the rail, so what opens is what the rail lists first.
  const [tab, setTab] = useState<Tab>(TABS[0].id);
  const { t } = useTranslation();
  // Drives the slide-in: false on first paint, flipped true after mount.
  const [shown, setShown] = useState(false);

  useEffect(() => {
    setShown(true);
  }, []);

  // A modal drawer: Esc closes it unless a layer opened over it takes the key.
  const titleId = useId();
  const dialog = useDialog({ onClose, labelledBy: titleId });

  return (
    <div
      className="fixed inset-0 z-50 bg-void/70 backdrop-blur-sm"
      onClick={onClose}
    >
      {/* Drawer: slides in from the left, spans 80% of the viewport width. */}
      <div
        {...dialog}
        onClick={(e) => e.stopPropagation()}
        className={`absolute inset-y-0 left-0 flex h-full w-[80vw] max-w-[1280px] border-r border-line bg-surface shadow-card transition-transform duration-300 ease-out ${
          shown ? 'translate-x-0' : '-translate-x-full'
        }`}
      >
        {/* Vertical tab rail. */}
        <nav className="flex w-64 shrink-0 flex-col border-r border-line bg-sidebar">
          <div className="flex items-center gap-2 px-5 py-5">
            <h2 id={titleId} className="font-display text-lg font-semibold text-ink">{t('settings.title')}</h2>
          </div>
          <div className="flex flex-1 flex-col gap-0.5 px-3">
            {TABS.map((tabDef) => {
              const active = tab === tabDef.id;
              const Icon = tabDef.icon;
              return (
                <button
                  key={tabDef.id}
                  onClick={() => setTab(tabDef.id)}
                  aria-current={active ? 'page' : undefined}
                  className={`flex items-center gap-3 border-l-2 px-3 py-2.5 text-left text-sm transition ${
                    active
                      ? 'border-accent bg-elevated font-medium text-ink'
                      : 'border-transparent text-muted hover:bg-elevated/50 hover:text-ink'
                  }`}
                >
                  <Icon className="h-4 w-4 shrink-0" />
                  <span>{t(tabDef.tKey)}</span>
                </button>
              );
            })}
          </div>
        </nav>

        {/* Close button, floating top-right of the drawer. */}
        <button
          onClick={onClose}
          className="absolute right-4 top-4 z-10 grid h-9 w-9 place-items-center text-muted transition hover:bg-elevated hover:text-ink"
          aria-label={t('common.close')}
        >
          <CloseIcon className="h-5 w-5" />
        </button>

        {/* Content for the active tab. */}
        <section className="flex-1 overflow-y-auto px-8 py-7">
          <div className="mx-auto max-w-3xl">
            {tab === 'system' && <SystemTab sys={model.sys} />}
            {tab === 'app' && <AppTab model={model} onStartTour={onStartTour} />}
            {tab === 'metrics' && <MetricsTab model={model} />}
            {tab === 'about' && <AboutTab />}

            {model.error && <p className="mt-6 text-sm text-accent">{model.error}</p>}
          </div>
        </section>
      </div>
    </div>
  );
}
