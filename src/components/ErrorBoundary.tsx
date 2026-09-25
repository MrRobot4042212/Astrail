// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

// A render throw used to unmount the whole tree and leave a blank window. The
// boundary logs it (app log, through Rust) and shows a way out instead.
//
// `scope="app"` sits at the root of every window and also owns the global
// error/rejection forwarding. `scope="view"` wraps one screen, so a broken detail
// view leaves the sidebar alive; it resets itself when `resetKey` changes, i.e.
// when the user navigates somewhere else.
import { Component, useState, type ErrorInfo, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { installGlobalErrorReporting, reportError } from '@/lib/errorReport';
import { exportDiagnostics } from '@/lib/tauri';
import { failureText } from '@/i18n/failureText';

type Scope = 'app' | 'view';

type Props = {
  scope: Scope;
  resetKey?: string;
  children: ReactNode;
};

type State = { error: Error | null };

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };
  private uninstall: (() => void) | null = null;

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidMount() {
    if (this.props.scope === 'app') this.uninstall = installGlobalErrorReporting();
  }

  componentWillUnmount() {
    this.uninstall?.();
    this.uninstall = null;
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    reportError(error, `render (${this.props.scope})${info.componentStack ?? ''}`);
  }

  componentDidUpdate(previous: Props) {
    if (this.state.error && previous.resetKey !== this.props.resetKey) {
      this.setState({ error: null });
    }
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <ErrorFallback
        scope={this.props.scope}
        message={error.message}
        onRetry={() => this.setState({ error: null })}
      />
    );
  }
}

function ErrorFallback({
  scope,
  message,
  onRetry,
}: {
  scope: Scope;
  message: string;
  onRetry: () => void;
}) {
  const { t } = useTranslation();
  const [exported, setExported] = useState<string | null>(null);

  const exportLog = () => {
    exportDiagnostics()
      .then(setExported)
      .catch((e) => setExported(failureText(e)));
  };

  const button =
    'border border-line bg-elevated px-4 py-2 text-sm font-medium text-ink transition hover:border-accent/40';

  return (
    <div
      role="alert"
      className={`flex flex-1 flex-col items-center justify-center gap-4 bg-void p-8 text-center ${
        scope === 'app' ? 'h-screen w-screen' : 'min-h-0'
      }`}
    >
      <p className="text-lg font-semibold text-ink">{t('errors.boundaryTitle')}</p>
      <p className="max-w-md text-sm leading-relaxed text-muted">{t('errors.boundaryBody')}</p>
      <p className="max-w-md break-words font-mono text-xs text-muted">{message}</p>
      <div className="flex flex-wrap justify-center gap-2">
        <button className={button} onClick={onRetry}>
          {t('errors.boundaryRetry')}
        </button>
        <button className={button} onClick={() => window.location.reload()}>
          {t('errors.boundaryReload')}
        </button>
        <button className={button} onClick={exportLog}>
          {t('settings.aDiagnosticsExport')}
        </button>
      </div>
      {exported && <p className="max-w-md break-all text-xs text-muted">{exported}</p>}
    </div>
  );
}
