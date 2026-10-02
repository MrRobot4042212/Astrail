// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import type { Metadata } from 'next';
import './fonts.css';
import './globals.css';
import { I18nProvider } from '@/i18n/I18nProvider';
import { ErrorBoundary } from '@/components/ErrorBoundary';

// Oxanium is the UI font (body + display); Source Code Pro for monospace bits.
// Both are declared in fonts.css and ship with the app: nothing is fetched at
// build time or at run time.

export const metadata: Metadata = {
  title: 'Astrail',
  description: 'Tu biblioteca unificada de juegos y apps',
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="es" className="dark">
      <body className="font-sans no-select">
        <I18nProvider>
          <ErrorBoundary scope="app">{children}</ErrorBoundary>
        </I18nProvider>
      </body>
    </html>
  );
}
