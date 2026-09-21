// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { describe, expect, it } from 'vitest';
import { adminWouldHelp, cpuTempAccess, fpsAccess } from './metricsAccess';

const a = (elevated: boolean, etw: boolean, pawnio: boolean) => ({ elevated, etw, pawnio });

describe('metrics access', () => {
  it('reads FPS without admin for Performance Log Users', () => {
    expect(fpsAccess(a(false, true, false))).toBe('group');
    expect(fpsAccess(a(false, false, false))).toBe('blocked');
    expect(fpsAccess(a(true, true, false))).toBe('admin');
  });

  it('needs admin and PawnIO for the CPU temperature', () => {
    expect(cpuTempAccess(a(true, true, true))).toBe('ok');
    expect(cpuTempAccess(a(true, true, false))).toBe('needsPawnio');
    expect(cpuTempAccess(a(false, true, true))).toBe('needsAdmin');
    expect(cpuTempAccess(a(false, false, false))).toBe('needsBoth');
  });

  it('offers the admin restart only when admin adds something', () => {
    expect(adminWouldHelp(a(true, true, false))).toBe(false);
    expect(adminWouldHelp(a(false, true, true))).toBe(true);
    expect(adminWouldHelp(a(false, false, false))).toBe(true);
  });
});
