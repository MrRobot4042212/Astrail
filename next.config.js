// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

/** @type {import('next').NextConfig} */
const nextConfig = {
  // Tauri serves a static bundle — no Node server at runtime.
  output: 'export',
  // next/image optimization needs a server, so disable it for static export.
  images: { unoptimized: true },
};

module.exports = nextConfig;
