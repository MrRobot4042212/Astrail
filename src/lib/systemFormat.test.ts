// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { describe, expect, it } from 'vitest';
import { fmtMem, GPU_KIND_KEYS } from './systemFormat';
import { en } from '@/i18n/en';
import { es } from '@/i18n/es';

describe('fmtMem', () => {
  it('prints megabytes below one gigabyte', () => {
    expect(fmtMem(0)).toBe('0 MB');
    expect(fmtMem(512)).toBe('512 MB');
    expect(fmtMem(1023)).toBe('1023 MB');
  });

  it('switches to gigabytes at exactly 1024 MB', () => {
    expect(fmtMem(1024)).toBe('1.0 GB');
  });

  it('keeps one decimal below 10 GB', () => {
    expect(fmtMem(8192)).toBe('8.0 GB');
    expect(fmtMem(8000)).toBe('7.8 GB');
    expect(fmtMem(10239)).toBe('10.0 GB');
  });

  it('drops the decimal from 10 GB up', () => {
    expect(fmtMem(10240)).toBe('10 GB');
    expect(fmtMem(16384)).toBe('16 GB');
    expect(fmtMem(24564)).toBe('24 GB');
  });
});

describe('GPU_KIND_KEYS', () => {
  const lookup = (catalog: unknown, key: string): unknown =>
    key.split('.').reduce<unknown>((node, part) => (node as Record<string, unknown> | undefined)?.[part], catalog);

  it('points at keys both catalogs define', () => {
    for (const key of Object.values(GPU_KIND_KEYS)) {
      expect(typeof lookup(en, key), `en: ${key}`).toBe('string');
      expect(typeof lookup(es, key), `es: ${key}`).toBe('string');
    }
  });
});
