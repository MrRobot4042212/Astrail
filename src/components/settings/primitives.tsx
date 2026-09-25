// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

// Building blocks shared by every settings tab and card. Presentation only: no
// state, no IPC, no translation lookups (callers pass translated strings).

import type { ReactNode } from 'react';

export function TabHeader({ title, desc }: { title: string; desc: string }) {
  return (
    <div className="mb-6">
      <h3 className="font-display text-xl font-semibold text-ink">{title}</h3>
      <p className="mt-1 text-sm text-muted">{desc}</p>
    </div>
  );
}

export function Card({
  title,
  control,
  children,
}: {
  title: string;
  control?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="border border-line bg-elevated/30 p-4">
      <div className="mb-2 flex items-center justify-between gap-3">
        <p className="text-sm font-medium text-ink">{title}</p>
        {control}
      </div>
      {children}
    </div>
  );
}

export function Button({
  children,
  onClick,
  disabled,
  className = '',
}: {
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={`w-full border border-line bg-elevated px-4 py-2.5 text-sm font-medium text-ink transition hover:border-accent/40 disabled:opacity-50 ${className}`}
    >
      {children}
    </button>
  );
}

/** `label` is the switch's accessible name: a bare switch is announced as
 *  "switch, on" with nothing saying what it switches. */
export function Toggle({
  on,
  onClick,
  label,
  disabled,
}: {
  on: boolean;
  onClick: () => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      role="switch"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      className={`relative h-6 w-11 shrink-0 border transition-colors disabled:opacity-50 ${
        on ? 'border-accent bg-accent' : 'border-line bg-elevated'
      }`}
    >
      <span
        className={`absolute top-1/2 h-4 w-4 -translate-y-1/2 transition-all ${
          on ? 'left-6 bg-primary-foreground' : 'left-1 bg-muted'
        }`}
      />
    </button>
  );
}

export function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <dt className="shrink-0 text-muted">{label}</dt>
      <dd className="truncate text-right text-ink">{value}</dd>
    </div>
  );
}

export function ListItem({ left, right }: { left: ReactNode; right: string }) {
  return (
    <div className="flex items-center justify-between border border-line bg-elevated px-3 py-2 text-sm">
      <span className="flex min-w-0 items-center truncate text-ink">{left}</span>
      <span className="ml-2 shrink-0 tabular-nums text-muted">{right}</span>
    </div>
  );
}

export function Tag({ children, accent }: { children: ReactNode; accent?: boolean }) {
  return (
    <span className={`ml-1.5 text-[10px] ${accent ? 'text-accent' : 'text-muted'}`}>
      · {children}
    </span>
  );
}

/** Segmented button group for selecting from a fixed set of options. */
export function SegmentedControl<T extends string | number>({
  options,
  value,
  onChange,
}: {
  options: { label: string; value: T }[];
  value: T;
  onChange: (v: T) => void;
}) {
  return (
    <div className="flex overflow-hidden border border-line">
      {options.map((o) => (
        <button
          key={String(o.value)}
          onClick={() => onChange(o.value)}
          className={`flex-1 px-2 py-1.5 text-xs transition ${
            value === o.value
              ? 'bg-accent font-medium text-primary-foreground'
              : 'bg-elevated text-muted hover:text-ink'
          }`}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
