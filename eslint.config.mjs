// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

// ESLint 9 flat config.
//
// `npm run lint` used to be `next lint`, which Next 16 removed — so the project
// shipped with no linter at all, and nothing caught (for example) the missing
// `useCallback`s that silently defeated `GameCard`'s `memo` for the whole grid.
// `react-hooks/exhaustive-deps` is an **error** here for exactly that reason.

import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import reactHooks from 'eslint-plugin-react-hooks';
import nextPlugin from '@next/eslint-plugin-next';
import globals from 'globals';

// House rules no published plugin covers.
const astrail = {
  rules: {
    // A link inside the app navigates the launcher's own webview away from it:
    // the webview is the app. External pages open through `openExternal`, which
    // validates the URL in Rust. The rule for that existed only in review, and a
    // credit link in the sidebar slipped past it (2026-09-27 audit, X-G3).
    'no-anchor': {
      meta: {
        type: 'problem',
        messages: {
          anchor:
            'No <a> in the app: open external pages with openExternal from src/lib/tauri.ts (a link navigates the webview itself).',
        },
        schema: [],
      },
      create(context) {
        return {
          JSXOpeningElement(node) {
            if (node.name.type === 'JSXIdentifier' && node.name.name === 'a') {
              context.report({ node, messageId: 'anchor' });
            }
          },
        };
      },
    },
  },
};

export default tseslint.config(
  {
    ignores: [
      'out/**',
      '.next/**',
      'node_modules/**',
      'src-tauri/target/**',
      'docs/**',
      'next-env.d.ts',
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ['src/**/*.{ts,tsx}'],
    languageOptions: {
      globals: { ...globals.browser, ...globals.es2022 },
      parserOptions: { ecmaFeatures: { jsx: true } },
    },
    plugins: {
      'react-hooks': reactHooks,
      '@next/next': nextPlugin,
      astrail,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      ...nextPlugin.configs.recommended.rules,

      'astrail/no-anchor': 'error',

      // A missing dependency is how a memoized callback goes stale, and a
      // changing one is how memoization stops working. Both are bugs here.
      'react-hooks/exhaustive-deps': 'error',

      // House rules: no `any`, no `@ts-ignore`.
      '@typescript-eslint/no-explicit-any': 'error',
      '@typescript-eslint/ban-ts-comment': 'error',
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
      ],

      // The design system is token-based and the global radius is 0. These catch
      // the drift a review would otherwise have to spot by eye.
      //
      // `warn`, not `error`, on purpose: the codebase predates the rule and has
      // ~60 existing hits (dialogs, TopBar, Onboarding). Fixing them is a visual
      // change that deserves its own pass with eyes on the result, not a blind
      // sweep — promote this to `error` once that pass lands.
      'no-restricted-syntax': [
        'warn',
        {
          selector:
            "JSXAttribute[name.name='className'] Literal[value=/rounded-(sm|md|lg|xl|2xl|3xl|full)/]",
          message:
            'Global border radius is 0 by design: drop the rounded-* class instead of re-adding corners.',
        },
        {
          selector:
            "JSXAttribute[name.name='className'] Literal[value=/#[0-9a-fA-F]{6}|rgba?[(]/]",
          message:
            'Use the semantic Tailwind tokens (bg-surface, text-muted, ring…), not a raw color.',
        },
      ],

      // The IPC contract lives in two files: commands in `src/lib/tauri.ts`,
      // events in `src/lib/events.ts` (whose names a Rust test checks). A call
      // made anywhere else escapes both the types and that check.
      'no-restricted-imports': [
        'error',
        {
          paths: [
            {
              name: '@tauri-apps/api/core',
              importNames: ['invoke'],
              message: 'Call commands through the typed wrappers in src/lib/tauri.ts.',
            },
            {
              name: '@tauri-apps/api/event',
              message: 'Listen through onEvent from src/lib/events.ts.',
            },
            {
              name: '@tauri-apps/api',
              importNames: ['core', 'event'],
              message: 'Use src/lib/tauri.ts (commands) and src/lib/events.ts (events).',
            },
          ],
        },
      ],
    },
  },
  {
    // The two files that own the IPC boundary.
    files: ['src/lib/tauri.ts', 'src/lib/events.ts'],
    rules: { 'no-restricted-imports': 'off' },
  },
  {
    // Node-side tooling and config files.
    files: ['*.{js,mjs,ts}', 'scripts/**/*.{js,mjs}'],
    languageOptions: { globals: globals.node },
  },
);
