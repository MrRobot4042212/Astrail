// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { Overlay } from '@/components/Overlay';

/**
 * Entry document for the in-game `overlay` window.
 *
 * The overlay used to open `index.html`, the same document as the launcher, and
 * decide which tree to render from the window label at runtime. That meant the
 * whole launcher bundle — the library grid, the home screen, the fuzzy search,
 * both translation catalogs — was downloaded, parsed and evaluated to draw one
 * settings panel, on the user's machine, **while a game is running**. That is the
 * one moment the app promised not to exist.
 *
 * `output: 'export'` emits this route as its own document and Next code-splits
 * per route, so the overlay window now loads only what it renders.
 */
export default function OverlayWindow() {
  return <Overlay />;
}
