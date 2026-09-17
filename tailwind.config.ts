// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import type { Config } from 'tailwindcss';

/** Helper: a theme color backed by a CSS variable, with Tailwind alpha support. */
const v = (name: string) => `rgb(var(--${name}) / <alpha-value>)`;

const config: Config = {
  darkMode: 'class',
  content: ['./src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        // Semantic tokens (shadcn-style) for new code.
        background: v('background'),
        foreground: v('foreground'),
        sidebar: v('sidebar'),
        primary: {
          DEFAULT: v('primary'),
          foreground: v('primary-foreground'),
        },
        info: v('info'),
        destructive: {
          DEFAULT: v('destructive'),
          foreground: v('destructive-foreground'),
        },
        border: v('border'),
        ring: v('ring'),
        popover: v('popover'),

        // Legacy aliases mapped onto the palette so existing component classes
        // keep working (void/surface/elevated/line/ink/muted/accent). The app's
        // primary action colour is the red `--primary`.
        void: v('background'),
        surface: v('surface'),
        elevated: v('elevated'),
        line: v('border'),
        ink: v('foreground'),
        muted: v('muted-foreground'),
        accent: {
          DEFAULT: v('primary'),
          soft: v('primary-soft'),
        },
      },
      fontFamily: {
        display: ['var(--font-oxanium)', 'system-ui', 'sans-serif'],
        sans: ['var(--font-oxanium)', 'system-ui', 'sans-serif'],
        mono: ['var(--font-mono)', 'ui-monospace', 'monospace'],
      },
      boxShadow: {
        card: '0px 2px 5px 0px rgb(0 0 0 / 0.55), 0px 6px 16px -6px rgb(0 0 0 / 0.6)',
        glow: '0 0 0 1px rgb(var(--ring) / 0.6), 0 14px 40px -18px rgb(var(--ring) / 0.35)',
      },
      // Modernist look: no rounded corners anywhere (sharp edges, --radius: 0).
      borderRadius: {
        none: '0',
        sm: '0',
        DEFAULT: '0',
        md: '0',
        lg: '0',
        xl: '0',
        '2xl': '0',
        '3xl': '0',
        full: '0',
        xl2: '0',
      },
    },
  },
  plugins: [],
};

export default config;
