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
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      ...nextPlugin.configs.recommended.rules,

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
    },
  },
  {
    // Node-side tooling and config files.
    files: ['*.{js,mjs,ts}', 'scripts/**/*.{js,mjs}'],
    languageOptions: { globals: globals.node },
  },
);
