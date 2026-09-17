import { describe, expect, it } from 'vitest';
import { recordShortcut, type ShortcutKey } from './shortcuts';

function key(partial: Partial<ShortcutKey> & Pick<ShortcutKey, 'key' | 'code'>): ShortcutKey {
  return { ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, ...partial };
}

describe('recordShortcut', () => {
  it('cancels on Escape instead of saving it as a hotkey', () => {
    expect(recordShortcut(key({ key: 'Escape', code: 'Escape' }))).toEqual({ kind: 'cancel' });
    expect(recordShortcut(key({ key: 'Escape', code: 'Escape', ctrlKey: true }))).toEqual({ kind: 'cancel' });
  });

  it('keeps recording while only modifiers are held', () => {
    expect(recordShortcut(key({ key: 'Control', code: 'ControlLeft', ctrlKey: true }))).toEqual({ kind: 'pending' });
  });

  it('refuses bare keys and Shift+letter', () => {
    expect(recordShortcut(key({ key: 'F9', code: 'F9' }))).toEqual({ kind: 'needs-modifier' });
    expect(recordShortcut(key({ key: 'A', code: 'KeyA', shiftKey: true }))).toEqual({ kind: 'needs-modifier' });
  });

  it('builds combinations the backend accepts', () => {
    expect(recordShortcut(key({ key: 'F9', code: 'F9', ctrlKey: true, shiftKey: true }))).toEqual({
      kind: 'combo',
      value: 'CommandOrControl+Shift+F9',
    });
    expect(recordShortcut(key({ key: 'o', code: 'KeyO', altKey: true }))).toEqual({ kind: 'combo', value: 'Alt+O' });
    expect(recordShortcut(key({ key: '5', code: 'Digit5', metaKey: true }))).toEqual({ kind: 'combo', value: 'Super+5' });
    expect(recordShortcut(key({ key: 'F5', code: 'F5', shiftKey: true }))).toEqual({ kind: 'combo', value: 'Shift+F5' });
  });
});
