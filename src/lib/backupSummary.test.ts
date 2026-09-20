// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { describe, expect, it } from 'vitest';
import { backupDate, backupIsEmpty, backupParts } from './backupSummary';
import type { BackupSummary } from './types';

const summary = (over: Partial<BackupSummary> = {}): BackupSummary => ({
  file_name: 'astrail-backup-2026-09-19.json',
  created: 1_789_000_000,
  app_version: '0.3.0',
  manual_apps: 0,
  played_games: 0,
  favorites: 0,
  hidden: 0,
  categories: 0,
  covers: 0,
  includes_settings: false,
  ...over,
});

describe('backupParts', () => {
  it('lists only what the file holds, in a fixed order', () => {
    const parts = backupParts(summary({ covers: 2, manual_apps: 1, favorites: 7 }));
    expect(parts).toEqual([
      { key: 'aBackupManual', count: 1 },
      { key: 'aBackupFavorites', count: 7 },
      { key: 'aBackupCovers', count: 2 },
    ]);
  });

  it('never shows a zero or a broken count', () => {
    expect(backupParts(summary({ hidden: 0, categories: Number.NaN, covers: -1 }))).toEqual([]);
  });
});

describe('backupIsEmpty', () => {
  it('is empty with no counted part and no settings', () => {
    expect(backupIsEmpty(summary())).toBe(true);
  });

  it('is not empty when it only carries settings', () => {
    expect(backupIsEmpty(summary({ includes_settings: true }))).toBe(false);
  });

  it('is not empty when it carries any data', () => {
    expect(backupIsEmpty(summary({ played_games: 3 }))).toBe(false);
  });
});

describe('backupDate', () => {
  it('reads unix seconds', () => {
    expect(backupDate(summary())?.getTime()).toBe(1_789_000_000_000);
  });

  it('has no date for a file that does not say, or says nonsense', () => {
    expect(backupDate(summary({ created: 0 }))).toBeNull();
    expect(backupDate(summary({ created: -5 }))).toBeNull();
    expect(backupDate(summary({ created: Number.MAX_VALUE }))).toBeNull();
  });
});
