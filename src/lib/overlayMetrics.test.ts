// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { describe, expect, it } from 'vitest';

import { en } from '../i18n/en';
import { es } from '../i18n/es';
import {
  GPU_BOUND_PCT,
  GRAPH_POINTS,
  OVERLAY_METRICS,
  PREVIEW_SAMPLE,
  gpuBound,
  graphBar,
  graphBaseline,
} from './overlayMetrics';

describe('frametime graph', () => {
  // Same numbers as `overlay::tests::graph_bars_scale_against_the_median`: the
  // preview must draw what the native HUD draws.
  it('scales against the median, so one hitch does not move the steady bars', () => {
    const points = [...Array<number>(59).fill(7), 40];
    const base = graphBaseline(points);
    expect(base).toBe(7);
    expect(graphBar(7, base)).toEqual({ height: 0.5, spike: false });
    expect(graphBar(10, base).spike).toBe(false);
    expect(graphBar(40, base)).toEqual({ height: 1, spike: true });
    expect(graphBar(0.1, base).height).toBeGreaterThan(0);
  });

  it('never produces a bar without a height', () => {
    expect(graphBaseline([])).toBe(0);
    expect(graphBar(7, 0).height).toBeGreaterThan(0);
    expect(Number.isFinite(graphBar(Number.NaN, 7).height)).toBe(true);
  });

  it('previews a full graph with a visible spike', () => {
    const graph = PREVIEW_SAMPLE.frametime_graph ?? [];
    expect(graph).toHaveLength(GRAPH_POINTS);
    const base = graphBaseline(graph);
    expect(graph.some((ft) => graphBar(ft, base).spike)).toBe(true);
  });
});

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

describe('gpu busy', () => {
  it('only a share at the threshold or above counts as a GPU limit', () => {
    expect(gpuBound(GPU_BOUND_PCT)).toBe(true);
    expect(gpuBound(100)).toBe(true);
    expect(gpuBound(GPU_BOUND_PCT - 0.1)).toBe(false);
    expect(gpuBound(0)).toBe(false);
    expect(gpuBound(Number.NaN)).toBe(false);
  });

  it('the preview shows the highlighted state', () => {
    expect(gpuBound(PREVIEW_SAMPLE.gpu_busy_pct ?? 0)).toBe(true);
  });
});
