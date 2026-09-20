// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { describe, expect, it } from 'vitest';

import { en } from '../i18n/en';
import { es } from '../i18n/es';
import {
  OVERLAY_METRICS,
} from './overlayMetrics';

describe('overlay metric switches', () => {
  it('each has a label in both catalogs', () => {
    for (const m of OVERLAY_METRICS) {
      const key = m.tKey.replace(/^metrics\./, '') as keyof typeof es.metrics;
      expect(es.metrics[key], m.tKey).toBeTruthy();
      expect(en.metrics[key], m.tKey).toBeTruthy();
    }
  });

  it('lists every switch once', () => {
    const keys = OVERLAY_METRICS.map((m) => m.key);
    expect(new Set(keys).size).toBe(keys.length);
  });
});
