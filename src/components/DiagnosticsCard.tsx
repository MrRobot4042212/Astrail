// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

// Settings → "Diagnóstico": writes the log, the crash reports and a system
// summary into one text file and opens its folder. Nothing leaves the machine;
// the user decides whether to attach the file to a bug report.
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { exportDiagnostics } from '@/lib/tauri';
import { Button, Card } from './settings/primitives';
import { failureText } from '@/i18n/failureText';

type State =
  | { kind: 'idle' }
  | { kind: 'working' }
  | { kind: 'done'; path: string }
  | { kind: 'failed'; reason: string };

export function DiagnosticsCard() {
  const { t } = useTranslation();
  const [state, setState] = useState<State>({ kind: 'idle' });

  const run = () => {
    setState({ kind: 'working' });
    exportDiagnostics()
      .then((path) => setState({ kind: 'done', path }))
      .catch((e) => setState({ kind: 'failed', reason: failureText(e) }));
  };

  return (
    <Card title={t('settings.aDiagnostics')}>
      <p className="mb-3 text-xs leading-relaxed text-muted">{t('settings.aDiagnosticsBody')}</p>
      <Button onClick={run} disabled={state.kind === 'working'}>
        {state.kind === 'working' ? t('settings.aWorking') : t('settings.aDiagnosticsExport')}
      </Button>
      {state.kind === 'done' && (
        <p className="mt-2 break-all text-xs text-muted">
          {t('settings.aDiagnosticsDone', { path: state.path })}
        </p>
      )}
      {state.kind === 'failed' && (
        <p className="mt-2 break-all text-xs text-destructive">
          {t('settings.aDiagnosticsFailed', { reason: state.reason })}
        </p>
      )}
    </Card>
  );
}
