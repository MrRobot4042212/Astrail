// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { describe, expect, it, vi } from 'vitest';

vi.mock('./tauri', () => ({ reportFrontendError: vi.fn(() => Promise.resolve()) }));

import { createDeduper, describeError } from './errorReport';

describe('describeError', () => {
  it('keeps the name, the message and the stack of an Error on one line', () => {
    const error = new TypeError('cover is undefined');
    const line = describeError(error, 'render');
    expect(line.startsWith('render: TypeError: cover is undefined')).toBe(true);
    expect(line).not.toMatch(/[\r\n]/);
    // The message is part of a V8 stack already; it must not be repeated.
    expect(line.split('cover is undefined').length - 1).toBe(1);
  });

  it('describes values that are not errors', () => {
    expect(describeError('plain text')).toBe('plain text');
    expect(describeError({ code: 7 })).toBe('{"code":7}');
    expect(describeError(undefined)).toBe('undefined');
  });

  it('survives a value that cannot be serialized', () => {
    const loop: Record<string, unknown> = {};
    loop.self = loop;
    expect(describeError(loop)).toBe('[object Object]');
  });

  it('caps the length so a huge stack cannot flood the IPC channel', () => {
    expect(describeError('x'.repeat(10_000)).length).toBeLessThanOrEqual(1501);
  });
});

describe('createDeduper', () => {
  it('drops an immediate repeat and lets a different message through', () => {
    const shouldSend = createDeduper();
    expect(shouldSend('a')).toBe(true);
    expect(shouldSend('a')).toBe(false);
    expect(shouldSend('b')).toBe(true);
    expect(shouldSend('a')).toBe(true);
  });
});
