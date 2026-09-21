// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

// Settings → "Copia de tus datos": export what a rescan cannot rebuild (manual
// apps, play time, favorites, categories, hidden entries, chosen covers, settings)
// to one file, and bring it back on another install.
//
// Both file dialogs are opened by Rust and the picked path never reaches this
// component: an import is "pick → read the summary → confirm", and the confirm
// acts on the file Rust already validated. The confirmation is inline rather than
// a modal on purpose: this card lives inside the settings dialog, which owns
// Escape, and a second layer of key handling over it would double-fire.
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  applyUserDataBackup,
  discardUserDataBackup,
  exportUserData,
  pickUserDataBackup,
} from '@/lib/tauri';
import { backupDate, backupIsEmpty, backupParts } from '@/lib/backupSummary';
import type { BackupExportReport, BackupSummary } from '@/lib/types';
import { Button, Card } from './settings/primitives';

type State =
  | { kind: 'idle' }
  | { kind: 'working' }
  | { kind: 'exported'; report: BackupExportReport }
  | { kind: 'picked'; summary: BackupSummary }
  | { kind: 'imported'; safetyCopy: string }
  | { kind: 'failed'; reason: string };

export function DataBackupCard() {
  const { t, i18n } = useTranslation();
  const [state, setState] = useState<State>({ kind: 'idle' });
  // A double click on "replace" must not start two imports: the second would find
  // nothing pending and paint an error over the first one's result.
  const applying = useRef(false);

  // Closing the settings with a backup picked forgets it.
  useEffect(() => () => void discardUserDataBackup().catch(() => undefined), []);

  const dateFormat = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' }),
    [i18n.language],
  );

  const fail = (e: unknown) => setState({ kind: 'failed', reason: String(e) });

  const runExport = () => {
    setState({ kind: 'working' });
    exportUserData()
      .then((report) => setState(report ? { kind: 'exported', report } : { kind: 'idle' }))
      .catch(fail);
  };

  const runPick = () => {
    setState({ kind: 'working' });
    pickUserDataBackup()
      .then((summary) => setState(summary ? { kind: 'picked', summary } : { kind: 'idle' }))
      .catch(fail);
  };

  const runApply = () => {
    if (applying.current) return;
    applying.current = true;
    setState({ kind: 'working' });
    applyUserDataBackup()
      .then((report) => setState({ kind: 'imported', safetyCopy: report.safety_copy }))
      .catch(fail)
      .finally(() => {
        applying.current = false;
      });
  };

  const cancelPick = () => {
    setState({ kind: 'idle' });
    discardUserDataBackup().catch(() => undefined);
  };

  const busy = state.kind === 'working';

  return (
    <Card title={t('settings.aBackup')}>
      <p className="mb-3 text-xs leading-relaxed text-muted">{t('settings.aBackupBody')}</p>
      <p className="mb-3 text-xs leading-relaxed text-muted">{t('settings.aBackupAuto')}</p>

      {state.kind !== 'picked' && (
        <div className="flex flex-col gap-2 sm:flex-row">
          <Button onClick={runExport} disabled={busy}>
            {busy ? t('settings.aWorking') : t('settings.aBackupExport')}
          </Button>
          <Button onClick={runPick} disabled={busy}>
            {t('settings.aBackupImport')}
          </Button>
        </div>
      )}

      {state.kind === 'picked' && <Picked summary={state.summary} dateFormat={dateFormat} onApply={runApply} onCancel={cancelPick} />}

      {state.kind === 'exported' && (
        <div className="mt-2 space-y-1 text-xs text-muted" role="status">
          <p className="break-all">{t('settings.aBackupExported', { path: state.report.path })}</p>
          {state.report.covers_skipped > 0 && (
            <p className="text-destructive">
              {t('settings.aBackupCoversSkipped', { count: state.report.covers_skipped })}
            </p>
          )}
          {state.report.unreadable.length > 0 && (
            <p className="break-all text-destructive">
              {t('settings.aBackupUnreadable', { files: state.report.unreadable.join(', ') })}
            </p>
          )}
        </div>
      )}
      {state.kind === 'imported' && (
        <p className="mt-2 break-all text-xs text-muted" role="status">
          {t('settings.aBackupImported', { path: state.safetyCopy })}
        </p>
      )}
      {state.kind === 'failed' && (
        <p className="mt-2 break-all text-xs text-destructive" role="alert">
          {t('settings.aBackupFailed', { reason: state.reason })}
        </p>
      )}
    </Card>
  );
}

function Picked({
  summary,
  dateFormat,
  onApply,
  onCancel,
}: {
  summary: BackupSummary;
  dateFormat: Intl.DateTimeFormat;
  onApply: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const date = backupDate(summary);
  const parts = backupParts(summary);
  const empty = backupIsEmpty(summary);

  return (
    <div className="border border-line bg-elevated p-3 text-xs">
      <p className="break-all font-medium text-ink">{summary.file_name}</p>
      <p className="mt-0.5 text-muted">
        {date ? t('settings.aBackupMadeOn', { date: dateFormat.format(date) }) : t('settings.aBackupUndated')}
        {summary.app_version && ` · Astrail ${summary.app_version}`}
      </p>

      {empty ? (
        <p className="mt-2 text-muted">{t('settings.aBackupEmpty')}</p>
      ) : (
        <ul className="mt-2 list-inside list-disc space-y-0.5 text-ink">
          {parts.map((part) => (
            <li key={part.key}>{t(`settings.${part.key}`, { count: part.count })}</li>
          ))}
          {summary.includes_settings && <li>{t('settings.aBackupSettings')}</li>}
        </ul>
      )}

      {!empty && <p className="mt-2 leading-relaxed text-destructive">{t('settings.aBackupWarn')}</p>}

      <div className="mt-3 flex flex-col gap-2 sm:flex-row">
        {!empty && (
          <button
            type="button"
            onClick={onApply}
            className="w-full bg-destructive px-4 py-2.5 text-sm font-medium text-destructive-foreground transition hover:bg-destructive/90"
          >
            {t('settings.aBackupConfirm')}
          </button>
        )}
        <Button onClick={onCancel}>{t('common.cancel')}</Button>
      </div>
    </div>
  );
}
