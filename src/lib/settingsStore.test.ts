// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { describe, expect, it } from 'vitest';

import { createSettingsStore, mergeSettings } from './settingsStore';
import type { AppSettings, AppSettingsPatch } from './types';

const BASE = {
  setup_completed: true,
  minimize_to_tray: true,
  language: 'system',
  discord_enabled: false,
  update_channel: 'stable',
  track_external_games: true,
  overlay: { enabled: false, interval_ms: 1000, show_fps: true },
  shortcuts: { spotlight: 'Alt+Space', overlay_toggle: 'Ctrl+Shift+F10', overlay_settings: 'Ctrl+Shift+F11' },
} as unknown as AppSettings;

function settings(patch: AppSettingsPatch = {}): AppSettings {
  return mergeSettings(BASE, patch);
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** A fake core: every `load` waits until the test answers it. */
function fakeCore() {
  const loads: ReturnType<typeof deferred<AppSettings>>[] = [];
  const saves: AppSettingsPatch[] = [];
  let failSave: unknown = null;
  let changed: () => void = () => {};
  const store = createSettingsStore({
    load: () => {
      const d = deferred<AppSettings>();
      loads.push(d);
      return d.promise;
    },
    save: async (patch) => {
      saves.push(patch);
      if (failSave) throw failSave;
    },
    watch: (cb) => {
      changed = cb;
    },
  });
  return {
    store,
    loads,
    saves,
    failNextSaves: (error: unknown) => {
      failSave = error;
    },
    announceChange: () => changed(),
  };
}

const flush = () => new Promise((r) => setTimeout(r, 0));

describe('settings store', () => {
  it('reads once for every component of the window, and again on each change', async () => {
    const core = fakeCore();
    let renders = 0;
    for (let i = 0; i < 4; i++) core.store.subscribe(() => renders++);
    expect(core.loads).toHaveLength(1);
    core.loads[0].resolve(settings());
    await flush();
    expect(core.store.getSnapshot().settings?.language).toBe('system');
    expect(renders).toBe(4);

    core.announceChange();
    expect(core.loads).toHaveLength(2);
    core.loads[1].resolve(settings({ language: 'es' }));
    await flush();
    expect(core.store.getSnapshot().settings?.language).toBe('es');
  });

  it('never lets a read that started before a change put the old value back', async () => {
    // Regression (G8): the settings dialog kept its own copy, read once. Here the
    // overlay hotkey turns the HUD on while a read that started earlier is still
    // on its way: its stale "off" must not land on top of the change.
    const core = fakeCore();
    core.store.subscribe(() => {});
    core.loads[0].resolve(settings());
    await flush();

    core.announceChange(); // a read starts…
    await core.store.patch({ overlay: { enabled: true } }); // …the user changes something
    expect(core.store.getSnapshot().settings?.overlay.enabled).toBe(true);

    core.loads[1].resolve(settings()); // the old read answers late, HUD still off
    await flush();
    expect(core.store.getSnapshot().settings?.overlay.enabled).toBe(true);

    core.announceChange(); // the core's own announcement of the change
    core.loads[2].resolve(settings({ overlay: { enabled: true } }));
    await flush();
    expect(core.store.getSnapshot().settings?.overlay.enabled).toBe(true);
  });

  it('drops an older read that answers after a newer one', async () => {
    const core = fakeCore();
    core.store.subscribe(() => {});
    core.announceChange();
    core.loads[1].resolve(settings({ language: 'en' }));
    await flush();
    core.loads[0].resolve(settings({ language: 'es' }));
    await flush();
    expect(core.store.getSnapshot().settings?.language).toBe('en');
  });

  it('puts back what the core has when a change fails, and rejects', async () => {
    const core = fakeCore();
    core.store.subscribe(() => {});
    core.loads[0].resolve(settings());
    await flush();

    core.failNextSaves(new Error('disk full'));
    const saving = core.store.patch({ update_channel: 'beta' });
    expect(core.store.getSnapshot().settings?.update_channel).toBe('beta'); // shown at once
    await flush();
    core.loads[1].resolve(settings()); // the re-read after the failure
    await expect(saving).rejects.toThrow('disk full');
    expect(core.store.getSnapshot().settings?.update_channel).toBe('stable');
  });

  it('merges nested settings key by key, like the core does', () => {
    const next = mergeSettings(BASE, { overlay: { interval_ms: 500 }, language: 'es' });
    expect(next.overlay).toEqual({ enabled: false, interval_ms: 500, show_fps: true });
    expect(next.language).toBe('es');
    expect(next.shortcuts).toBe(BASE.shortcuts);
    expect(BASE.overlay.interval_ms).toBe(1000); // the input is untouched
  });

  it('reports a failed first read, but keeps the settings when a re-read fails', async () => {
    const core = fakeCore();
    core.store.subscribe(() => {});
    core.loads[0].reject(new Error('ipc down'));
    await flush();
    expect(core.store.getSnapshot()).toEqual({ settings: null, error: new Error('ipc down') });

    core.announceChange();
    core.loads[1].resolve(settings());
    await flush();
    core.announceChange();
    core.loads[2].reject(new Error('ipc down'));
    await flush();
    expect(core.store.getSnapshot().settings).not.toBeNull();
    expect(core.store.getSnapshot().error).toBeNull();
  });
});
