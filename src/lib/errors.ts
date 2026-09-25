// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

// Turning a failed command into text the user can read.
//
// Every command rejects with an `AppError` (`{ code, detail }`, see
// src-tauri/src/error.rs): the code has a translation under `errors.code.*`, the
// detail is English and only fills the gaps the translation leaves. Showing the
// rejection with `String(e)` prints "[object Object]"; always go through here.
import type { TFunction } from 'i18next';
import type { AppError, ErrorCode } from './types';

// `satisfies` makes this list fail to compile when a code is added in Rust and
// the generated `ErrorCode` grows without it.
const CODES = [
  'not_found',
  'invalid_input',
  'autostart_unavailable',
  'backup_invalid',
  'backup_changed',
  'launch_failed',
  'io',
  'internal',
] as const satisfies readonly ErrorCode[];
type Missing = Exclude<ErrorCode, (typeof CODES)[number]>;
const _everyCode: Missing extends never ? true : Missing = true;
void _everyCode;

export function isAppError(e: unknown): e is AppError {
  if (typeof e !== 'object' || e === null) return false;
  const { code, detail } = e as Record<string, unknown>;
  return typeof detail === 'string' && (CODES as readonly unknown[]).includes(code);
}

/** The catalog key and interpolation values for a rejection of any shape. */
export function errorMessageKey(e: unknown): { key: string; detail: string } {
  if (isAppError(e)) return { key: `errors.code.${e.code}`, detail: e.detail };
  const detail = e instanceof Error ? e.message : String(e);
  return { key: 'errors.code.internal', detail };
}

/** User-facing text for a failed command. */
export function errorText(e: unknown, t: TFunction): string {
  const { key, detail } = errorMessageKey(e);
  return t(key, { detail });
}
