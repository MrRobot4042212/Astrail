// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import type { Metadata } from 'next';
import { Oxanium, Source_Code_Pro } from 'next/font/google';
import './globals.css';
import { I18nProvider } from '@/i18n/I18nProvider';

// Oxanium is the UI font (body + display); Source Code Pro for monospace bits.
// Explicit weights: without them next/font ships the whole variable axis, which
// is a much bigger file than the four weights this UI actually uses.
const oxanium = Oxanium({
  subsets: ['latin'],
  weight: ['400', '500', '600', '700'],
  variable: '--font-oxanium',
  display: 'swap',
});

const mono = Source_Code_Pro({
  subsets: ['latin'],
  weight: ['400', '600'],
  variable: '--font-mono',
  display: 'swap',
});

export const metadata: Metadata = {
  title: 'Meteor',
  description: 'Tu biblioteca unificada de juegos y apps',
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="es" className={`dark ${oxanium.variable} ${mono.variable}`}>
      <body className="font-sans no-select">
        <I18nProvider>{children}</I18nProvider>
      </body>
    </html>
  );
}
