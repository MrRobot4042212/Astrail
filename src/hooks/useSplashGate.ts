// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

import { useCallback, useEffect, useState } from 'react';

const FADE_MS = 500;

/**
 * Mounts the scan splash while `active`, and keeps it mounted for the length of
 * its fade-out once it should hide, so it dissolves into the library instead of
 * popping away. `skip` hides it for the scan in progress; `rearm` lets the next
 * scan show it again.
 */
export function useSplashGate(active: boolean) {
  const [skipped, setSkipped] = useState(false);
  const [mounted, setMounted] = useState(false);
  const [exiting, setExiting] = useState(false);

  const showing = active && !skipped;
  useEffect(() => {
    if (showing) {
      setMounted(true);
      setExiting(false);
    } else if (mounted) {
      setExiting(true);
      const t = window.setTimeout(() => setMounted(false), FADE_MS);
      return () => window.clearTimeout(t);
    }
  }, [showing, mounted]);

  const skip = useCallback(() => setSkipped(true), []);
  const rearm = useCallback(() => setSkipped(false), []);

  return { mounted, exiting, skip, rearm };
}
