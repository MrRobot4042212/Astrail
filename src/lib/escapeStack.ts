// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

// Escape closes the top-most open layer, and only that one.
//
// Every dialog, menu and overlay used to add its own `keydown` listener on
// `window`, so one Escape reached all of them: a confirmation opened over the
// settings closed both, and cancelling a shortcut recording (which consumes the
// key) closed the settings drawer too. Layers now push a handler here; one
// listener runs the most recently pushed one, and leaves alone a key another
// handler has already consumed (`defaultPrevented`). Use it through `useEscape`.

type Entry = { run: () => void };

const stack: Entry[] = [];
let listening = false;

/** Run the top-most handler. Returns whether there was one. */
export function dispatchEscape(): boolean {
  const top = stack[stack.length - 1];
  if (!top) return false;
  top.run();
  return true;
}

function onKeyDown(e: KeyboardEvent) {
  if (e.key !== 'Escape' || e.defaultPrevented) return;
  if (dispatchEscape()) {
    e.preventDefault();
    e.stopPropagation();
  }
}

/** Add a layer on top. Returns the function that removes it. */
export function pushEscape(run: () => void): () => void {
  const entry: Entry = { run };
  stack.push(entry);
  if (!listening && typeof window !== 'undefined') {
    window.addEventListener('keydown', onKeyDown);
    listening = true;
  }
  return () => {
    const i = stack.indexOf(entry);
    if (i >= 0) stack.splice(i, 1);
  };
}

/** How many layers are open (tests). */
export function escapeDepth(): number {
  return stack.length;
}
