/**
 * The color to persist when a native color picker settles on `next`, or `null` when
 * nothing changed (`<input type="color">` reports lowercase hex, stored values may
 * not be).
 *
 * The picker's `input` event fires on every drag tick, and each one used to run the
 * full settings round trip: read + write of the settings file, re-applying the HUD
 * config, Discord and re-registering every global shortcut on the main thread. The
 * dialog now previews locally and commits once, through this check.
 */
export function colorToCommit(next: string, committed: string): string | null {
  return next.toLowerCase() === committed.toLowerCase() ? null : next;
}
