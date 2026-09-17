// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { useTranslation } from 'react-i18next';
import { SearchIcon, ScanIcon, PlayIcon, BellIcon, HelpIcon } from './icons';

export type SortKey = 'name' | 'recent' | 'played';

/** i18n key for each sort option's label. */
export const SORT_LABEL_KEYS: Record<SortKey, string> = {
  name: 'topbar.sortName',
  recent: 'topbar.sortRecent',
  played: 'topbar.sortPlayed',
};

export function TopBar({
  query,
  setQuery,
  showingHome,
  sort,
  setSort,
  handleRescan,
  loading,
  setShowNotifications,
  setShowAdd,
  onStartTour,
}: {
  query: string;
  setQuery: (q: string) => void;
  showingHome: boolean;
  sort: SortKey;
  setSort: (s: SortKey) => void;
  handleRescan: () => void;
  loading: boolean;
  setShowNotifications: (s: boolean) => void;
  setShowAdd: (s: boolean) => void;
  /** Re-launch the guided product tour. */
  onStartTour: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="relative flex h-16 shrink-0 items-center justify-center border-b border-line px-6">
      {/* Search box centered */}
      <div data-tour="search" className="relative w-full max-w-xl transition-all duration-300">
        <SearchIcon className="absolute left-3.5 top-1/2 h-[18px] w-[18px] -translate-y-1/2 text-muted" />
        <input
          type="text"
          placeholder={t('topbar.searchPlaceholder')}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="w-full rounded-xl border border-line bg-elevated/50 py-2.5 pl-10 pr-4 text-sm text-ink outline-none transition hover:border-accent/40 focus:border-accent focus:bg-surface focus:shadow-[0_0_15px_rgba(223,79,79,0.1)]"
        />
        {/* Decorative subtle border glow when focused */}
        <div className="pointer-events-none absolute inset-0 -z-10 rounded-xl bg-accent/5 opacity-0 transition-opacity peer-focus:opacity-100" />
      </div>

      {/* Right side controls (Notifications and settings/actions) */}
      <div className="absolute right-6 flex items-center gap-2">
        <button
          onClick={onStartTour}
          title={t('topbar.tour')}
          className="grid h-10 w-10 place-items-center rounded-lg text-muted transition hover:bg-elevated hover:text-accent"
        >
          <HelpIcon className="h-[18px] w-[18px]" />
        </button>

        <button
          onClick={() => setShowNotifications(true)}
          title={t('topbar.notifications')}
          className="group relative grid h-10 w-10 place-items-center text-muted transition hover:text-accent"
        >
          <BellIcon className="h-5 w-5" />
          <span className="absolute right-2 top-2 h-2 w-2 rounded-full bg-accent opacity-0 transition-opacity group-hover:opacity-100" />
        </button>

        {!showingHome && !query && (
          <select
            value={sort}
            onChange={(e) => setSort(e.target.value as SortKey)}
            className="h-10 cursor-pointer appearance-none rounded-lg border border-transparent bg-transparent pl-3 pr-8 text-sm font-medium text-muted outline-none transition hover:bg-elevated hover:text-ink focus:border-line"
            style={{
              backgroundImage:
                "url(\"data:image/svg+xml;charset=utf-8,%3Csvg xmlns='http://www.w3.org/2000/svg' fill='none' viewBox='0 0 20 20'%3E%3Cpath stroke='%23a0a0a0' stroke-linecap='round' stroke-linejoin='round' stroke-width='1.5' d='m6 8 4 4 4-4'/%3E%3C/svg%3E\")",
              backgroundPosition: 'right 0.25rem center',
              backgroundRepeat: 'no-repeat',
              backgroundSize: '1.25em 1.25em',
            }}
          >
            {(Object.keys(SORT_LABEL_KEYS) as SortKey[]).map((k) => (
              <option key={k} value={k} className="bg-surface text-ink">
                {t(SORT_LABEL_KEYS[k])}
              </option>
            ))}
          </select>
        )}

        <button
          onClick={handleRescan}
          disabled={loading}
          title={t('topbar.scan')}
          className="grid h-10 w-10 place-items-center rounded-lg text-muted transition hover:bg-elevated hover:text-accent disabled:opacity-50"
        >
          <ScanIcon className={`h-[18px] w-[18px] ${loading ? 'animate-pulse' : ''}`} />
        </button>
        <button
          onClick={() => setShowAdd(true)}
          title={t('topbar.addManual')}
          className="grid h-10 w-10 place-items-center rounded-lg text-muted transition hover:bg-elevated hover:text-accent"
        >
          <div className="relative">
            <PlayIcon className="h-4 w-4" />
            <span className="absolute -bottom-1 -right-1 flex h-3 w-3 items-center justify-center rounded-full bg-surface text-[10px] font-bold text-ink">
              +
            </span>
          </div>
        </button>
      </div>
    </div>
  );
}
