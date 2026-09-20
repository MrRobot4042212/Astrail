// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

// What the library area shows when it has no grid to show: the loading
// skeleton, the empty/error state, and the transient toast.

/** Card-shaped placeholders while the first scan has nothing to paint yet. */
export function SkeletonGrid() {
  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-6">
      {Array.from({ length: 12 }).map((_, i) => (
        <div key={i} className="aspect-[2/3] animate-pulse border border-line bg-elevated" />
      ))}
    </div>
  );
}

export function Empty({
  title,
  body,
  action,
}: {
  title: string;
  body: string;
  action?: { label: string; onClick: () => void };
}) {
  return (
    <div className="grid h-full place-items-center text-center">
      <div className="max-w-sm">
        <h3 className="mb-2 font-display text-lg font-semibold text-ink">{title}</h3>
        <p className="mb-5 text-sm leading-relaxed text-muted">{body}</p>
        {action && (
          <button
            onClick={action.onClick}
            className="bg-accent px-4 py-2.5 text-sm font-semibold text-white hover:bg-accent-soft"
          >
            {action.label}
          </button>
        )}
      </div>
    </div>
  );
}

/** The message from `useToast`. The `role="status"` region stays mounted (empty,
 *  so it has no size) because a live region only announces changes made after
 *  it exists. */
export function Toast({ message }: { message: string | null }) {
  return (
    <div role="status" className="fixed bottom-14 left-1/2 -translate-x-1/2">
      {message && (
        <div className="border border-line bg-elevated px-4 py-2.5 text-sm text-ink shadow-card">{message}</div>
      )}
    </div>
  );
}
