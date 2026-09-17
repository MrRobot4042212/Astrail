'use client';

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { getAppSettings } from '@/lib/tauri';
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
  const [spotlightShortcut, setSpotlightShortcut] = useState(DEFAULT_SHORTCUTS.spotlight);

  useEffect(() => {
    const fetchSettings = async () => {
      try {
        const settings = await getAppSettings();
        if (settings.shortcuts?.spotlight) {
          setSpotlightShortcut(settings.shortcuts.spotlight);
        }
      } catch {
        // ignore
      }
    };

    fetchSettings();
    const un = listen('settings-updated', fetchSettings);
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  return (
    <footer data-tour="footer" className="flex shrink-0 items-center gap-5 overflow-x-auto border-t border-line bg-sidebar px-4 py-2 text-[11px] text-muted">
      <Shortcut keys={formatShortcut(spotlightShortcut, t('common.keySpace'))} label={t('footer.spotlight')} />
      <Shortcut keys={[t('footer.rightClick')]} label={t('footer.actions')} />
      <Shortcut keys={['Ctrl', t('footer.click')]} label={t('footer.selectMultiple')} />
      <Shortcut keys={[t('footer.drag')]} label={t('footer.categorizeReorder')} />
      <Shortcut keys={['↑', '↓', '↵']} label={t('footer.navigateSearch')} />
      <Shortcut keys={['Esc']} label={t('footer.closeBack')} />
    </footer>
  );
}
