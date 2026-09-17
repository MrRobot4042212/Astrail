'use client';

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation, Trans } from 'react-i18next';
import { getAppSettings } from '@/lib/tauri';
import { DEFAULT_SHORTCUTS, formatShortcut } from '@/lib/shortcuts';
import type { ShortcutsSettings } from '@/lib/types';
import {
  PlayIcon,
  StarIcon,
  TagIcon,
  ImageIcon,
  EyeOffIcon,
  SearchIcon,
  GearIcon,
  HomeIcon,
  GridIcon,
} from './icons';

/* -------------------------------------------------------------------------- */
/*  Guided product tour                                                        */
/*                                                                            */
/*  A coachmark tour that highlights real UI elements (box-shadow "hole" over  */
/*  the target's rect) and explains each feature in a floating tooltip.        */
/*  Features that only exist after an interaction are handled two ways         */
/*  (the "mixto" approach): the cheap & safe ones are auto-driven — the tour    */
/*  opens the real context menu / detail page and highlights it — while the     */
/*  The in-game and app-wide ones (Spotlight, overlay, drag&drop, multi-      */
/*  shown with an inline illustration instead of forcing the app into them.     */
/*                                                                            */
/*  Anchoring uses `data-tour="…"` attributes on the real components, so the    */
/*  tour doesn't depend on fragile class selectors.                            */
/* -------------------------------------------------------------------------- */

type Placement = 'top' | 'bottom' | 'left' | 'right' | 'center';

interface Step {
  id: string;
  title: string;
  body: React.ReactNode;
  /** Element to highlight (CSS selector). Absent → centered, full dim. */
  selector?: string;
  placement?: Placement;
  /** Which library view must be showing for this step. */
  view?: 'home' | 'all';
  /** Drive the UI into a transient state (open menu / detail) before measuring. */
  enter?: () => void | Promise<void>;
  /** Don't scroll the target into view (e.g. an already-open popup). */
  noScroll?: boolean;
  /** A static illustration shown in the tooltip (for describe-only steps). */
  illustration?: React.ReactNode;
  padding?: number;
}

type Rect = { top: number; left: number; width: number; height: number };

const raf = () => new Promise<void>((r) => requestAnimationFrame(() => r()));
const tick = async () => {
  await raf();
  await raf();
};

/** Poll for an element until it exists or we time out. */
function waitFor(selector: string, timeout = 1000): Promise<HTMLElement | null> {
  return new Promise((resolve) => {
    const found = document.querySelector<HTMLElement>(selector);
    if (found) return resolve(found);
    const start = performance.now();
    const iv = setInterval(() => {
      const el = document.querySelector<HTMLElement>(selector);
      if (el || performance.now() - start > timeout) {
        clearInterval(iv);
        resolve(el ?? null);
      }
    }, 40);
  });
}

function Kbd({ children }: { children?: React.ReactNode }) {
  return (
    <kbd className="border border-line bg-elevated px-1.5 py-0.5 font-mono text-[11px] leading-none text-ink">
      {children}
    </kbd>
  );
}

/** Render a shortcut's key tokens as <Kbd> chips joined with "+". */
function Keys({ tokens }: { tokens: string[] }) {
  return (
    <span className="inline-flex items-center gap-1 align-text-bottom">
      {tokens.map((t, i) => (
        <span key={i} className="inline-flex items-center gap-1">
          {i > 0 && <span className="text-muted">+</span>}
          <Kbd>{t}</Kbd>
        </span>
      ))}
    </span>
  );
}

/* --- inline illustrations for the describe-only steps --------------------- */

function SpotlightIllu({ tokens, label }: { tokens: string[]; label: string }) {
  return (
    <div className="flex flex-col items-center gap-2 border border-line bg-void/40 px-4 py-3">
      <Keys tokens={tokens} />
      <div className="flex w-full items-center gap-2 border border-accent/40 bg-surface px-2 py-1.5">
        <SearchIcon className="h-3.5 w-3.5 text-muted" />
        <span className="text-[11px] text-muted">{label}</span>
      </div>
    </div>
  );
}

function MultiselectIllu() {
  return (
    <div className="flex items-center justify-center gap-2 border border-line bg-void/40 px-4 py-3">
      {[true, false, true].map((on, i) => (
        <div key={i} className="relative h-12 w-8 border border-line bg-elevated">
          <span
            className={`absolute left-0.5 top-0.5 grid h-3.5 w-3.5 place-items-center border ${
              on ? 'border-accent bg-accent text-white' : 'border-white/40'
            }`}
          >
            {on && (
              <svg viewBox="0 0 24 24" className="h-2.5 w-2.5" fill="none" stroke="currentColor" strokeWidth="4">
                <path d="M5 13l4 4L19 7" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            )}
          </span>
        </div>
      ))}
    </div>
  );
}

function OverlayIllu() {
  return (
    <div className="flex justify-center border border-line bg-void/40 px-4 py-3">
      <div className="space-y-0.5 border border-white/10 bg-black/70 px-2.5 py-1.5 font-mono text-[10px]">
        <div className="flex justify-between gap-4">
          <span className="text-muted">FPS</span>
          <span className="font-semibold text-accent">204</span>
        </div>
        <div className="flex justify-between gap-4">
          <span className="text-muted">GPU</span>
          <span className="font-semibold text-accent">62%</span>
        </div>
        <div className="flex justify-between gap-4">
          <span className="text-muted">CPU °C</span>
          <span className="font-semibold text-emerald-400">58°</span>
        </div>
      </div>
    </div>
  );
}

export function GuidedTour({
  onFinish,
  setView,
  resetUi,
}: {
  /** Close the tour (finished or skipped). */
  onFinish: () => void;
  /** Switch the library to a view the step needs (home grid vs full grid). */
  setView: (f: 'home' | 'all') => void;
  /** Return the app to a clean slate (close menu / detail / selection). */
  resetUi: () => void;
}) {
  const { t } = useTranslation();
  const CARD = '[data-tour="game-card"]';

  // Keybindings shown in the tour must reflect the user's custom shortcuts (or
  // our default), so load them and format for display. Falls back to the current
  // defaults until settings arrive.
  const [sc, setSc] = useState<ShortcutsSettings>(DEFAULT_SHORTCUTS);
  useEffect(() => {
    getAppSettings()
      .then((s) => s.shortcuts && setSc(s.shortcuts))
      .catch(() => {});
  }, []);
  // Rebuilt only when the shortcuts load. The enter() closures don't depend on
  // them, and the step effect keys on `idx`, so this never re-drives a step.
  const steps = useMemo<Step[]>(() => {
    const space = t('common.keySpace');
    const spotKeys = formatShortcut(sc.spotlight, space);
    const overlayToggleKeys = formatShortcut(sc.overlay_toggle, space);
    const overlaySettingsKeys = formatShortcut(sc.overlay_settings, space);
    return [
    {
      id: 'welcome',
      title: t('tour.welcomeTitle'),
      body: t('tour.welcomeBody'),
      view: 'home',
      placement: 'center',
    },
    {
      id: 'sidebar',
      title: t('tour.sidebarTitle'),
      body: t('tour.sidebarBody'),
      selector: '[data-tour="sidebar"]',
      placement: 'right',
      view: 'home',
    },
    {
      id: 'home',
      title: t('tour.homeTitle'),
      body: t('tour.homeBody'),
      selector: '[data-tour="home"]',
      placement: 'top',
      view: 'home',
    },
    {
      id: 'search',
      title: t('tour.searchTitle'),
      body: t('tour.searchBody'),
      selector: '[data-tour="search"]',
      placement: 'bottom',
      view: 'home',
    },
    {
      id: 'card',
      title: t('tour.cardTitle'),
      body: (
        <Trans
          i18nKey="tour.cardBody"
          components={[
            <b key="0" />,
            <StarIcon key="1" className="inline h-3.5 w-3.5 align-text-bottom" />,
            <TagIcon key="2" className="inline h-3.5 w-3.5 align-text-bottom" />,
            <ImageIcon key="3" className="inline h-3.5 w-3.5 align-text-bottom" />,
            <EyeOffIcon key="4" className="inline h-3.5 w-3.5 align-text-bottom" />,
          ]}
        />
      ),
      selector: CARD,
      placement: 'right',
      view: 'all',
    },
    {
      id: 'context',
      title: t('tour.contextTitle'),
      body: t('tour.contextBody'),
      selector: '[data-tour="context-menu"]',
      placement: 'right',
      view: 'all',
      noScroll: true,
      enter: async () => {
        const card = await waitFor(CARD);
        if (!card) return;
        card.scrollIntoView({ block: 'center' });
        await tick();
        const b = card.getBoundingClientRect();
        card.dispatchEvent(
          new MouseEvent('contextmenu', {
            bubbles: true,
            cancelable: true,
            clientX: b.left + b.width / 2,
            clientY: b.top + b.height / 2,
          }),
        );
      },
    },
    {
      id: 'detail',
      title: t('tour.detailTitle'),
      body: t('tour.detailBody'),
      selector: '[data-tour="detail"]',
      placement: 'center',
      view: 'all',
      enter: async () => {
        const card = await waitFor(CARD);
        if (card) card.click();
      },
    },
    {
      id: 'multiselect',
      title: t('tour.multiselectTitle'),
      body: <Trans i18nKey="tour.multiselectBody" components={[<Kbd key="0" />]} />,
      selector: CARD,
      placement: 'right',
      view: 'all',
      illustration: <MultiselectIllu />,
    },
    {
      id: 'drag',
      title: t('tour.dragTitle'),
      body: t('tour.dragBody'),
      selector: '[data-tour="categories"]',
      placement: 'right',
      view: 'all',
    },
    {
      id: 'spotlight',
      title: t('tour.spotlightTitle'),
      body: (
        <Trans i18nKey="tour.spotlightBody" components={[<Keys key="0" tokens={spotKeys} />]} />
      ),
      selector: '[data-tour="footer"]',
      placement: 'top',
      view: 'all',
      illustration: <SpotlightIllu tokens={spotKeys} label={t('tour.searchLaunch')} />,
    },
    {
      id: 'overlay',
      title: t('tour.overlayTitle'),
      body: (
        <Trans
          i18nKey="tour.overlayBody"
          components={[
            <Keys key="0" tokens={overlayToggleKeys} />,
            <Keys key="1" tokens={overlaySettingsKeys} />,
          ]}
        />
      ),
      placement: 'center',
      view: 'all',
      illustration: <OverlayIllu />,
    },
    {
      id: 'settings',
      title: t('tour.settingsTitle'),
      body: t('tour.settingsBody'),
      selector: '[data-tour="settings-btn"]',
      placement: 'right',
      view: 'all',
    },
    ];
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sc, t]);

  const [idx, setIdx] = useState(0);
  const [rect, setRect] = useState<Rect | null>(null);
  const [ready, setReady] = useState(false);
  const [pos, setPos] = useState<{ top: number; left: number }>({ top: -9999, left: -9999 });
  const tipRef = useRef<HTMLDivElement>(null);
  // The element currently highlighted, kept so we can re-measure on resize.
  const targetRef = useRef<HTMLElement | null>(null);

  const step = steps[idx];

  const measureFrom = useCallback((el: HTMLElement | null): Rect | null => {
    if (!el) return null;
    const b = el.getBoundingClientRect();
    if (b.width === 0 && b.height === 0) return null;
    return { top: b.top, left: b.left, width: b.width, height: b.height };
  }, []);

  // Enter a step: clean slate → switch view → drive the UI → measure the target.
  useEffect(() => {
    let cancelled = false;
    setReady(false);
    targetRef.current = null;

    (async () => {
      resetUi();
      if (step.view) setView(step.view);
      await tick();
      if (cancelled) return;

      if (step.enter) {
        await step.enter();
        if (cancelled) return;
      }

      let r: Rect | null = null;
      if (step.selector) {
        const el = await waitFor(step.selector);
        if (cancelled) return;
        if (el) {
          if (!step.noScroll) {
            el.scrollIntoView({ block: 'center' });
            await tick();
            if (cancelled) return;
          }
          targetRef.current = el;
          r = measureFrom(el);
        }
      }
      setRect(r);
      setReady(true);
    })();

    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [idx]);

  // Position the tooltip relative to the target once both are known/measured.
  useLayoutEffect(() => {
    if (!ready) return;
    const tip = tipRef.current;
    if (!tip) return;
    const tw = tip.offsetWidth;
    const th = tip.offsetHeight;
    const M = 16;
    const pad = 12;
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    if (!rect || step.placement === 'center') {
      setPos({ left: (vw - tw) / 2, top: (vh - th) / 2 });
      return;
    }
    let left: number;
    let top: number;
    switch (step.placement) {
      case 'right':
        left = rect.left + rect.width + M;
        top = rect.top;
        break;
      case 'left':
        left = rect.left - tw - M;
        top = rect.top;
        break;
      case 'top':
        left = rect.left;
        top = rect.top - th - M;
        break;
      case 'bottom':
      default:
        left = rect.left;
        top = rect.top + rect.height + M;
        break;
    }
    left = Math.max(pad, Math.min(left, vw - tw - pad));
    top = Math.max(pad, Math.min(top, vh - th - pad));
    setPos({ left, top });
  }, [ready, rect, idx, step.placement]);

  // Keep the highlight aligned if the window is resized.
  useEffect(() => {
    const onResize = () => setRect(measureFrom(targetRef.current));
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, [measureFrom]);

  const last = idx === steps.length - 1;
  const next = useCallback(() => {
    if (last) {
      resetUi();
      onFinish();
    } else {
      setIdx((i) => i + 1);
    }
  }, [last, onFinish, resetUi]);
  const prev = useCallback(() => setIdx((i) => Math.max(0, i - 1)), []);
  const finish = useCallback(() => {
    resetUi();
    onFinish();
  }, [onFinish, resetUi]);

  // Keyboard navigation.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        finish();
      } else if (e.key === 'ArrowRight' || e.key === 'Enter') {
        next();
      } else if (e.key === 'ArrowLeft') {
        prev();
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [finish, next, prev]);

  const pad = step.padding ?? 8;

  return (
    <div className="fixed inset-0 z-[80]">
      {/* Click blocker: swallows interaction with the dimmed app. Transparent —
          the actual dimming is the box-shadow on the hole below (or the full
          dim when there's no target). */}
      <div
        className="absolute inset-0"
        onClick={(e) => e.stopPropagation()}
        onContextMenu={(e) => e.preventDefault()}
      />

      {/* Spotlight hole over the target, or a full dim for centered steps. */}
      {ready && rect ? (
        <div
          className="pointer-events-none absolute"
          style={{
            left: rect.left - pad,
            top: rect.top - pad,
            width: rect.width + pad * 2,
            height: rect.height + pad * 2,
            boxShadow: '0 0 0 9999px rgba(8,8,10,0.78)',
            borderRadius: 4,
            transition: 'all 0.35s cubic-bezier(0.4, 0, 0.2, 1)',
          }}
        >
          <div className="absolute inset-0 ring-2 ring-accent/70" style={{ borderRadius: 4 }} />
        </div>
      ) : (
        <div className="pointer-events-none absolute inset-0" style={{ background: 'rgba(8,8,10,0.82)' }} />
      )}

      {/* Tooltip card */}
      <div
        ref={tipRef}
        className="absolute w-[340px] border border-line bg-surface p-5 shadow-card"
        style={{
          left: pos.left,
          top: pos.top,
          opacity: ready ? 1 : 0,
          transition: 'left 0.3s ease, top 0.3s ease, opacity 0.2s ease',
        }}
      >
        <div className="mb-3 flex items-center gap-2">
          <span className="grid h-7 w-7 place-items-center bg-accent text-white">
            <StepIcon id={step.id} />
          </span>
          <h3 className="font-display text-base font-semibold text-ink">{step.title}</h3>
        </div>

        {step.illustration && <div className="mb-3">{step.illustration}</div>}

        <p className="text-sm leading-relaxed text-muted">{step.body}</p>

        {/* Progress dots */}
        <div className="mt-4 flex items-center gap-1.5">
          {steps.map((s, i) => (
            <span
              key={s.id}
              className={`h-1.5 transition-all ${
                i === idx ? 'w-5 bg-accent' : 'w-1.5 bg-line'
              }`}
            />
          ))}
        </div>

        <div className="mt-4 flex items-center justify-between">
          <button
            onClick={finish}
            className="text-xs text-muted transition hover:text-ink"
          >
            {t('tour.skip')}
          </button>
          <div className="flex items-center gap-2">
            <span className="mr-1 text-xs tabular-nums text-muted">
              {idx + 1} / {steps.length}
            </span>
            {idx > 0 && (
              <button
                onClick={prev}
                className="border border-line px-3 py-1.5 text-sm text-ink transition hover:bg-elevated"
              >
                {t('tour.back')}
              </button>
            )}
            <button
              onClick={next}
              className="bg-accent px-4 py-1.5 text-sm font-semibold text-white transition hover:bg-accent-soft"
            >
              {last ? t('tour.start') : t('onboarding.next')}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

/** Small per-step glyph for the tooltip header. */
function StepIcon({ id }: { id: string }) {
  const c = 'h-4 w-4';
  switch (id) {
    case 'sidebar':
      return <GridIcon className={c} />;
    case 'home':
      return <HomeIcon className={c} />;
    case 'search':
      return <SearchIcon className={c} />;
    case 'card':
    case 'detail':
      return <PlayIcon className={c} />;
    case 'context':
    case 'drag':
      return <TagIcon className={c} />;
    case 'multiselect':
      return <StarIcon className={c} />;
    case 'overlay':
      return <ImageIcon className={c} />;
    case 'settings':
      return <GearIcon className={c} />;
    default:
      return <StarIcon className={c} />;
  }
}
