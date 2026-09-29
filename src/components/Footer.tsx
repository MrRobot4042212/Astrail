// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useTranslation } from 'react-i18next';
import { useAppSettings } from '@/hooks/useAppSettings';
import { DEFAULT_SHORTCUTS, formatShortcut } from '@/lib/shortcuts';

/** A small key/badge chip. */
function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="border border-line bg-elevated px-1.5 py-0.5 font-mono text-[10px] leading-none text-ink">
      {children}
    </kbd>
  );
}

function Shortcut({ keys, label }: { keys: React.ReactNode[]; label: string }) {
  return (
    <span className="flex shrink-0 items-center gap-1.5 border border-line bg-elevated px-1.5 py-0.5 font-mono text-[10px] leading-none text-ink">
      <span className="flex items-center gap-1">
        {keys.map((k, i) => (
          <Kbd key={i}>{k}</Kbd>
        ))}
      </span>
      <span className="text-muted">{label}</span>
    </span>
  );
}

/** Footer toolbar listing the app's keyboard/interaction shortcuts. */
export function Footer() {
  const { t } = useTranslation();
  const { settings } = useAppSettings();
  const spotlightShortcut = settings?.shortcuts?.spotlight || DEFAULT_SHORTCUTS.spotlight;

  return (
    // One row, most useful first: hints that do not fit wrap onto a second row
    // that the fixed height hides, instead of a horizontal scrollbar that hid the
    // last ones (1236 px of hints in 1072 px at the default window, D25).
    <footer
      data-tour="footer"
      className="flex h-9 shrink-0 flex-wrap content-start items-center gap-x-5 gap-y-3 overflow-hidden border-t border-line bg-sidebar px-4 py-2 text-[11px] text-muted"
    >
      <Shortcut keys={formatShortcut(spotlightShortcut, t('common.keySpace'))} label={t('footer.spotlight')} />
      <Shortcut keys={[t('footer.rightClick')]} label={t('footer.actions')} />
      <Shortcut keys={['Esc']} label={t('footer.closeBack')} />
      <Shortcut keys={['Ctrl', t('footer.click')]} label={t('footer.selectMultiple')} />
      <Shortcut keys={[t('footer.drag')]} label={t('footer.categorizeReorder')} />
      <Shortcut keys={['↑', '↓', '↵']} label={t('footer.navigateSearch')} />
    </footer>
  );
}
