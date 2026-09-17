// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import type { SVGProps } from 'react';

/** Bundled icon set users can pick from when creating a custom category. Each is
 *  a monochrome stroke icon (currentColor), matching the app's icon style. The
 *  chosen key is persisted per category and resolved here for rendering. */
type P = SVGProps<SVGSVGElement>;
const base = {
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 1.8,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
  viewBox: '0 0 24 24',
};

const make = (d: string) => (p: P) => (
  <svg {...base} {...p}>
    <path d={d} />
  </svg>
);

// Gaming-flavoured set mixed with general-purpose icons, in a deliberately mixed
// order for the picker grid. Add more by appending a `key: make('<path>')`.
export const CATEGORY_ICONS: Record<string, (p: P) => React.ReactElement> = {
  // --- Gaming ---
  gamepad: make('M7 8h10a4 4 0 014 4l-1 4a3 3 0 01-5 1l-1-1H10l-1 1a3 3 0 01-5-1l-1-4a4 4 0 014-4zM7 12h4M9 10v4M16 11h.01M18 13h.01'),
  controller: make('M3 9h18v9a1 1 0 01-1 1H4a1 1 0 01-1-1zM3 9l2-4h14l2 4M8 14h.01M16 14h.01'),
  joystick: make('M5 21h14M9 21l1-7M15 21l-1-7M12 3a3 3 0 100 6 3 3 0 000-6zM12 9v5'),
  dpad: make('M9 3h6v6h6v6h-6v6H9v-6H3V9h6z'),
  crosshair: make('M12 3v4M12 17v4M3 12h4M17 12h4M12 7a5 5 0 100 10 5 5 0 000-10z'),
  target: make('M12 4a8 8 0 100 16 8 8 0 000-16zM12 8a4 4 0 100 8 4 4 0 000-8zM12 12h.01'),
  // --- General ---
  star: make('M12 3.5l2.6 5.27 5.82.85-4.21 4.1.99 5.79L12 16.77l-5.2 2.74.99-5.79-4.21-4.1 5.82-.85z'),
  heart: make('M12 20S3 14.5 3 8.5A4 4 0 0112 6a4 4 0 019 2.5C21 14.5 12 20 12 20z'),
  // --- Gaming ---
  sword: make('M14.5 3H21v6.5l-9 9-3-3zM5 16l3 3M3 21l3-2'),
  shield: make('M12 3l7 3v5c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6z'),
  dice: make('M5 5h14v14H5zM9 9h.01M12 12h.01M15 15h.01'),
  puzzle: make('M14 4a2 2 0 10-4 0H6v4a2 2 0 100 4v4h4a2 2 0 104 0h4v-4a2 2 0 100-4V4h-4z'),
  trophy: make('M8 4h8v4a4 4 0 01-8 0zM8 6H5a3 3 0 003 3M16 6h3a3 3 0 01-3 3M10 14h4M9 20h6M12 14v4'),
  crown: make('M4 18h16M4 18l-1-9 5 4 4-7 4 7 5-4-1 9'),
  rocket: make('M5 15c-1 2-1 4-1 4s2 0 4-1m-3-3a8 8 0 0111-11 8 8 0 01-11 11zm6-6a1.5 1.5 0 100-3 1.5 1.5 0 000 3z'),
  ghost: make('M5 21v-9a7 7 0 0114 0v9l-2.5-2-2.5 2-2-2-2 2L7 19zM9.5 11h.01M14.5 11h.01'),
  skull: make('M6 11a6 6 0 1112 0v2a2 2 0 01-2 2v2H8v-2a2 2 0 01-2-2zM9.5 11h.01M14.5 11h.01M12 15v2'),
  // --- General ---
  fire: make('M12 3c1 3-2 4-2 7a2 2 0 004 0c0-1 0-2-.5-3 2 1 3.5 3 3.5 6a5 5 0 11-10 0c0-4 3-5 5-10z'),
  bolt: make('M13 2 4 14h6l-1 8 9-12h-6z'),
  // --- Gaming ---
  potion: make('M9 3h6M10 3v5L6.5 15A2.5 2.5 0 009 19h6a2.5 2.5 0 002.5-4L14 8V3M8 14h8'),
  chest: make('M4 9a2 2 0 012-2h12a2 2 0 012 2v9a1 1 0 01-1 1H5a1 1 0 01-1-1zM4 12h16M11 11h2v2h-2z'),
  coin: make('M12 4a8 8 0 100 16 8 8 0 000-16zM12 7a5 5 0 100 10 5 5 0 000-10zM12 10v4'),
  // --- General ---
  diamond: make('M6 3h12l3 6-9 12L3 9z'),
  sparkles: make('M12 4l1.5 4.5L18 10l-4.5 1.5L12 16l-1.5-4.5L6 10l4.5-1.5zM18.5 16l.6 1.8 1.9.6-1.9.6-.6 1.9-.6-1.9-1.9-.6 1.9-.6z'),
  flag: make('M5 21V4m0 0h11l-2 4 2 4H5'),
  bookmark: make('M6 3h12v18l-6-4-6 4z'),
  clock: make('M12 7v5l3 2M21 12a9 9 0 11-18 0 9 9 0 0118 0z'),
  // --- Gaming ---
  map: make('M9 4 3 6v14l6-2 6 2 6-2V4l-6 2-6-2zM9 4v14M15 6v14'),
  keyboard: make('M4 7h16a1 1 0 011 1v8a1 1 0 01-1 1H4a1 1 0 01-1-1V8a1 1 0 011-1zM7 10h.01M11 10h.01M15 10h.01M8 14h8'),
  headset: make('M5 14v-2a7 7 0 0114 0v2M4 14h3v5H5a1 1 0 01-1-1zM20 14h-3v5h2a1 1 0 001-1zM13 21h2'),
  wheel: make('M12 4a8 8 0 100 16 8 8 0 000-16zM12 10a2 2 0 100 4 2 2 0 000-4M12 4v6M6.5 17l4-3M17.5 17l-4-3'),
};

/** Stable ordering for the picker grid. */
export const CATEGORY_ICON_KEYS = Object.keys(CATEGORY_ICONS);
