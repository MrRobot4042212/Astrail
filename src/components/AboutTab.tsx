// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

// Settings → "Acerca de": the Appropriate Legal Notices the GPL asks an
// interactive program to display, plus the author attribution that the
// additional terms require every build to keep.
//
// The author name and the repository link come from Rust, not from the
// translation catalogs, so translating the screen cannot drop them.
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { aboutInfo, legalDocument, openExternal } from '@/lib/tauri';
import type { AboutInfo, LegalDocument } from '@/lib/types';
import { ArrowLeftIcon, BookIcon, AstrailIcon } from './icons';

const DOCUMENTS: { id: LegalDocument; tKey: string }[] = [
  { id: 'license', tKey: 'about.docLicense' },
  { id: 'additional-terms', tKey: 'about.docTerms' },
  { id: 'third-party-notices', tKey: 'about.docNotices' },
];

export function AboutTab() {
  const { t } = useTranslation();
  const [info, setInfo] = useState<AboutInfo | null>(null);
  const [open, setOpen] = useState<LegalDocument | null>(null);
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState('');

  useEffect(() => {
    aboutInfo()
      .then(setInfo)
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    if (!open) return;
    let alive = true;
    setText(null);
    setError('');
    legalDocument(open)
      .then((body) => {
        if (alive) setText(body);
      })
      .catch((e) => {
        if (alive) setError(String(e));
      });
    return () => {
      alive = false;
    };
  }, [open]);

  if (open) {
    const title = DOCUMENTS.find((d) => d.id === open)?.tKey ?? '';
    return (
      <div>
        <button
          onClick={() => setOpen(null)}
          className="mb-4 flex items-center gap-2 text-sm text-muted transition hover:text-ink"
        >
          <ArrowLeftIcon className="h-4 w-4" />
          <span>{t('about.back')}</span>
        </button>
        <h3 className="mb-4 font-display text-xl font-semibold text-ink">{t(title)}</h3>
        {error && <p className="text-sm text-accent">{error}</p>}
        {!error && text === null && <p className="text-sm text-muted">{t('about.loading')}</p>}
        {text !== null && (
          // The texts are already wrapped by their authors, so no re-wrapping:
          // reflowing a license is the one thing not to do to it.
          <pre className="max-h-[62vh] overflow-auto border border-line bg-elevated/30 p-4 font-mono text-xs leading-relaxed text-muted">
            {text}
          </pre>
        )}
      </div>
    );
  }

  return (
    <div>
      <div className="mb-6">
        <h3 className="font-display text-xl font-semibold text-ink">{t('about.title')}</h3>
        <p className="mt-1 text-sm text-muted">{t('about.desc')}</p>
      </div>

      {error && <p className="mb-4 text-sm text-accent">{error}</p>}

      <div className="border border-line bg-elevated/30 p-5">
        <div className="flex items-center gap-3">
          <AstrailIcon className="h-9 w-9 text-accent" />
          <div>
            <p className="font-display text-lg font-semibold text-ink">Astrail</p>
            <p className="text-sm text-muted">
              {info ? t('about.version', { version: info.version }) : '—'}
            </p>
          </div>
        </div>

        {info && (
          <div className="mt-4 space-y-2 border-t border-line pt-4">
            <p className="text-sm text-ink">{t('about.createdBy', { author: info.author })}</p>
            <button
              onClick={() => void openExternal(info.repository)}
              className="break-all text-left text-sm text-accent transition hover:underline"
            >
              {info.repository}
            </button>
          </div>
        )}
      </div>

      {info && !info.official && (
        <div className="mt-4 border border-accent/40 bg-elevated/30 p-4">
          <p className="text-sm font-medium text-accent">{t('about.unofficialTitle')}</p>
          <p className="mt-1 text-sm text-muted">{t('about.unofficialBody')}</p>
        </div>
      )}

      <div className="mt-4 border border-line bg-elevated/30 p-4">
        <p className="text-sm font-medium text-ink">
          {t('about.licenseTitle', { license: info?.license ?? 'GPL-3.0-only' })}
        </p>
        <p className="mt-1 text-sm text-muted">{t('about.licenseBody')}</p>
        <div className="mt-4 grid gap-2">
          {DOCUMENTS.map((doc) => (
            <button
              key={doc.id}
              onClick={() => setOpen(doc.id)}
              className="flex items-center gap-2 border border-line bg-elevated px-4 py-2.5 text-left text-sm font-medium text-ink transition hover:border-accent/40"
            >
              <BookIcon className="h-4 w-4 shrink-0 text-muted" />
              <span>{t(doc.tKey)}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
