// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

// What the import confirmation lists. Pure so it can be tested: the user agrees to
// replace their data based on exactly these lines.
import type { BackupSummary } from './types';

/** i18n key stems under `settings.`; each has `_one` / `_other` forms. */
export type BackupPartKey =
  | 'aBackupManual'
  | 'aBackupPlayed'
  | 'aBackupFavorites'
  | 'aBackupHidden'
  | 'aBackupCategories'
  | 'aBackupCovers';

export interface BackupPart {
  key: BackupPartKey;
  count: number;
}

/** The counted parts of a backup, in display order, leaving out what it does not
 *  hold. A `0` line would read as "this will be emptied", which is a different
 *  claim from "this is not in the file". */
export function backupParts(summary: BackupSummary): BackupPart[] {
  const all: BackupPart[] = [
    { key: 'aBackupManual', count: summary.manual_apps },
    { key: 'aBackupPlayed', count: summary.played_games },
    { key: 'aBackupFavorites', count: summary.favorites },
    { key: 'aBackupHidden', count: summary.hidden },
    { key: 'aBackupCategories', count: summary.categories },
    { key: 'aBackupCovers', count: summary.covers },
  ];
  return all.filter((part) => Number.isFinite(part.count) && part.count > 0);
}

/** True when importing the file would bring nothing the user can see. */
export function backupIsEmpty(summary: BackupSummary): boolean {
  return backupParts(summary).length === 0 && !summary.includes_settings;
}

/** `created` as a Date, or `null` when the file does not say (0) or says
 *  something no calendar holds. The value comes from the file, not from Astrail. */
export function backupDate(summary: BackupSummary): Date | null {
  const secs = summary.created;
  if (!Number.isFinite(secs) || secs <= 0) return null;
  const date = new Date(secs * 1000);
  return Number.isNaN(date.getTime()) ? null : date;
}
