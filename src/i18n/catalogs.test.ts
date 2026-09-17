// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { describe, expect, it } from 'vitest';
import { es } from './es';
import { en } from './en';

type Catalog = Record<string, unknown>;

/** Flatten a nested catalog to dotted keys, so a diff points at the exact entry. */
function keysOf(node: Catalog, prefix = ''): string[] {
  return Object.entries(node).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    return value && typeof value === 'object' && !Array.isArray(value)
      ? keysOf(value as Catalog, path)
      : [path];
  });
}

const esKeys = keysOf(es as Catalog).sort();
const enKeys = keysOf(en as Catalog).sort();

describe('i18n catalogs', () => {
  // The rule is 1:1 parity. A missing key does not fail the build
  // or the typecheck — it silently renders the raw key id in the UI, which is
  // exactly the kind of thing only a test catches.
  it('has no key present in es but missing in en', () => {
    expect(esKeys.filter((k) => !enKeys.includes(k))).toEqual([]);
  });

  it('has no key present in en but missing in es', () => {
    expect(enKeys.filter((k) => !esKeys.includes(k))).toEqual([]);
  });

  it('keeps plural variants in both catalogs', () => {
    const plurals = (keys: string[]) =>
      keys.filter((k) => k.endsWith('_one') || k.endsWith('_other'));
    expect(plurals(esKeys)).toEqual(plurals(enKeys));
  });

  it('has no empty strings', () => {
    const empties: string[] = [];
    const walk = (node: Catalog, prefix: string, lang: string) => {
      for (const [key, value] of Object.entries(node)) {
        const path = `${lang}:${prefix}${key}`;
        if (typeof value === 'string') {
          if (value.trim() === '') empties.push(path);
        } else if (value && typeof value === 'object') {
          walk(value as Catalog, `${prefix}${key}.`, lang);
        }
      }
    };
    walk(es as Catalog, '', 'es');
    walk(en as Catalog, '', 'en');
    expect(empties).toEqual([]);
  });

  it('uses the same interpolation placeholders in both languages', () => {
    const placeholders = (text: string) =>
      (text.match(/\{\{\s*[\w.]+\s*\}\}/g) ?? [])
        .map((p) => p.replace(/\s/g, ''))
        .sort();
    const get = (node: Catalog, path: string): unknown =>
      path.split('.').reduce<unknown>((acc, part) => (acc as Catalog)?.[part], node);

    const mismatches: string[] = [];
    for (const key of esKeys) {
      const a = get(es as Catalog, key);
      const b = get(en as Catalog, key);
      if (typeof a === 'string' && typeof b === 'string') {
        const [pa, pb] = [placeholders(a), placeholders(b)];
        if (JSON.stringify(pa) !== JSON.stringify(pb)) {
          mismatches.push(`${key}: es=${pa.join(',')} en=${pb.join(',')}`);
        }
      }
    }
    expect(mismatches).toEqual([]);
  });
});
