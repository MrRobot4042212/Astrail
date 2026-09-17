import type { ShortcutsSettings } from './types';

/**
 * The defaults, mirrored from `ShortcutsSettings` in `src-tauri/src/models.rs`.
 *
 * Every screen that shows a keybinding needs something to display until the real
 * settings arrive over IPC; each of them used to carry its own copy, so changing a
 * default in Rust left the UI announcing a shortcut that no longer existed. They
 * are modified combinations on purpose — a bare key is taken away from every
 * application on the machine, games included.
 */
export const DEFAULT_SHORTCUTS: ShortcutsSettings = {
  spotlight: 'Ctrl+Shift+F9',
  overlay_toggle: 'Ctrl+Shift+F10',
  overlay_settings: 'Ctrl+Shift+F11',
};

/**
 * Turn a stored shortcut string (Tauri global-shortcut form, e.g.
 * "Control+Shift+KeyO", "F9", "Control+Shift+Space") into human-readable key
 * tokens for display: ["Ctrl", "Shift", "O"] / ["F9"] / ["Ctrl", "Shift",
 * "Space"]. Used wherever we show a keybinding so it always reflects the
 * user's custom binding (or our default), never a hardcoded label. Pass
 * `t('common.keySpace')` as `spaceLabel`: it is the only token that is a word.
 */
export function formatShortcut(shortcut: string | undefined | null, spaceLabel = 'Space'): string[] {
  if (!shortcut) return [];
  return shortcut.split('+').map((part) => {
    if (part === 'CommandOrControl' || part === 'Control' || part === 'CmdOrCtrl') return 'Ctrl';
    if (part === 'Space') return spaceLabel;
    if (part === 'Meta' || part === 'Super') return 'Win';
    // Tauri encodes letter/digit keys as KeyO / Digit5; show the bare character.
    if (part.startsWith('Key') && part.length === 4) return part.slice(3);
    if (part.startsWith('Digit') && part.length === 6) return part.slice(5);
    return part;
  });
}

/** The subset of a `KeyboardEvent` the recorder reads (plain object in tests). */
export type ShortcutKey = Pick<KeyboardEvent, 'key' | 'code' | 'ctrlKey' | 'shiftKey' | 'altKey' | 'metaKey'>;

export type RecordResult =
  | { kind: 'cancel' }
  | { kind: 'pending' }
  | { kind: 'needs-modifier' }
  | { kind: 'combo'; value: string };

/**
 * Interpret one keydown while recording a global shortcut.
 *
 * Escape is checked first and always cancels: it used to fall into the generic
 * branch and be saved as `ESCAPE`, which the backend then registered as a bare,
 * machine-wide hotkey. A combination must carry Ctrl, Alt or Win (Shift alone only
 * with a function key), mirroring `is_safe_global_shortcut` in `src-tauri/src/lib.rs`.
 */
export function recordShortcut(e: ShortcutKey): RecordResult {
  if (e.key === 'Escape') return { kind: 'cancel' };
  if (['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) return { kind: 'pending' };

  const isFunctionKey = /^F([1-9]|1\d|2[0-4])$/.test(e.code);
  if (!(e.ctrlKey || e.altKey || e.metaKey || (e.shiftKey && isFunctionKey))) {
    return { kind: 'needs-modifier' };
  }

  const keys: string[] = [];
  if (e.ctrlKey) keys.push('CommandOrControl');
  if (e.shiftKey) keys.push('Shift');
  if (e.altKey) keys.push('Alt');
  if (e.metaKey) keys.push('Super');

  let key = e.code;
  if (key.startsWith('Key')) key = key.slice(3);
  else if (key.startsWith('Digit')) key = key.slice(5);
  else if (key !== 'Space') key = e.key.toUpperCase();
  keys.push(key);
  return { kind: 'combo', value: keys.join('+') };
}
