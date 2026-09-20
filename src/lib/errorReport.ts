// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

// Forwards webview failures to the app log. A packaged build has no devtools, so
// without this a render throw or a rejected promise is a blank window with no
// trace anywhere. Rust sanitizes the text and caps the count per run; this side
// only shapes the message and drops immediate repeats.
import { reportFrontendError } from './tauri';

/** Longest message sent over IPC; Rust truncates again on its side. */
const MAX_LENGTH = 1500;

/** One line describing anything that can be thrown or rejected with. */
export function describeError(value: unknown, context?: string): string {
  let text: string;
  if (value instanceof Error) {
    const stack = value.stack ?? '';
    // V8 stacks start with "Name: message"; keep it once, not twice.
    text = stack.includes(value.message) ? stack : `${value.name}: ${value.message} ${stack}`;
  } else if (typeof value === 'string') {
    text = value;
  } else {
    try {
      text = JSON.stringify(value) ?? String(value);
    } catch {
      text = String(value);
    }
  }
  const line = `${context ? `${context}: ` : ''}${text}`.replace(/\s+/g, ' ').trim();
  return line.length > MAX_LENGTH ? `${line.slice(0, MAX_LENGTH)}…` : line;
}

/** Decides whether a message is sent: not when it repeats the previous one. */
export function createDeduper(): (message: string) => boolean {
  let last = '';
  return (message) => {
    if (message === last) return false;
    last = message;
    return true;
  };
}

const shouldSend = createDeduper();

/** Log a webview failure. Never throws: reporting an error must not cause one. */
export function reportError(value: unknown, context?: string): void {
  const message = describeError(value, context);
  if (!shouldSend(message)) return;
  reportFrontendError(message).catch(() => {});
}

/** Forward uncaught errors and unhandled rejections. Returns the cleanup. */
export function installGlobalErrorReporting(): () => void {
  const onError = (e: ErrorEvent) => reportError(e.error ?? e.message, 'window.onerror');
  const onRejection = (e: PromiseRejectionEvent) => reportError(e.reason, 'unhandledrejection');
  window.addEventListener('error', onError);
  window.addEventListener('unhandledrejection', onRejection);
  return () => {
    window.removeEventListener('error', onError);
    window.removeEventListener('unhandledrejection', onRejection);
  };
}
