// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { patchSettings, useAppSettings } from '@/hooks/useAppSettings';
import { privacyUrl } from '@/lib/privacy';
import { getAutostart, openExternal, setAutostart } from '@/lib/tauri';
import { AstrailIcon } from './icons';
import { failureText } from '@/i18n/failureText';

export function Onboarding({
  onComplete,
}: {
  onComplete: () => void;
}) {
  const { t, i18n } = useTranslation();
  const SLIDES = [
    { title: t('onboarding.welcomeTitle'), description: t('onboarding.welcomeBody') },
    { title: t('onboarding.spotlightTitle'), description: t('onboarding.spotlightBody') },
    { title: t('onboarding.customizeTitle'), description: t('onboarding.customizeBody') },
    { title: t('onboarding.backgroundTitle'), description: t('onboarding.backgroundBody') },
  ];
  const [step, setStep] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  
  const [auto, setAuto] = useState<boolean>(true);
  // Only an installed copy can start with Windows; a build run from elsewhere
  // hides the switch instead of failing the whole setup on it.
  const [autoAvailable, setAutoAvailable] = useState(false);
  // Drafts, saved on the last slide: they follow the saved settings until the
  // user flips them (null = not touched yet).
  const { settings } = useAppSettings();
  const [trayChoice, setTray] = useState<boolean | null>(null);
  const [metricsChoice, setMetrics] = useState<boolean | null>(null);
  const [coversChoice, setCovers] = useState<boolean | null>(null);
  const tray = trayChoice ?? settings?.minimize_to_tray ?? true;
  const metrics = metricsChoice ?? settings?.overlay.enabled ?? false;
  // The cover lookup sends game names to IGDB. The library is not scanned until
  // this setup is finished, so switching it off here means nothing was ever sent.
  const covers = coversChoice ?? settings?.online_covers ?? true;

  useEffect(() => {
    getAutostart()
      .then((autostartRes) => {
        setAuto(autostartRes.enabled);
        setAutoAvailable(autostartRes.available);
      })
      .catch((e) => console.error(e));
  }, []);

  async function handleNext() {
    if (step < SLIDES.length - 1) {
      setStep(step + 1);
    } else {
      await finish();
    }
  }

  async function finish() {
    setBusy(true);
    setError(null);
    try {
      if (autoAvailable) await setAutostart(auto);
      await patchSettings({
        setup_completed: true,
        minimize_to_tray: tray,
        overlay: { enabled: metrics },
        online_covers: covers,
      });
      onComplete();
    } catch (e) {
      setError(failureText(e));
      setBusy(false);
    }
  }

  const isLast = step === SLIDES.length - 1;

  return (
    // Scrolls when the window is too short for the last slide: centred with no
    // way to scroll, its button ended up below the window (and the header above
    // it) at the default size once a fourth option was on screen.
    <div className="fixed inset-0 z-[100] overflow-y-auto bg-void text-ink">
      <div className="pointer-events-none fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 w-[800px] h-[800px] bg-accent/5 blur-[120px] rounded-full" />

      <div className="flex min-h-full flex-col items-center justify-center p-6">
      <div className="relative w-full max-w-3xl">
        {/* Header Icon */}
        <div className="mx-auto mb-6 flex h-20 w-20 items-center justify-center rounded-2xl bg-gradient-to-br from-accent to-accent-soft shadow-[0_0_40px_rgba(223,79,79,0.3)] transition-all duration-500">
          <AstrailIcon className="h-10 w-10 text-white" />
        </div>

        {/* Carousel Content */}
        <div className="text-center mb-10 h-32">
          <h1 className="font-display text-4xl font-bold tracking-tight text-white mb-3">
            {SLIDES[step].title}
          </h1>
          <p className="text-muted text-base leading-relaxed max-w-md mx-auto">
            {SLIDES[step].description}
          </p>
        </div>

        {/* Preferences (only shown on the last slide) */}
        {/* Two columns, so the four options and the button fit the default window. */}
        <div className={`grid grid-cols-2 gap-4 mb-10 transition-opacity duration-300 ${isLast ? 'opacity-100' : 'opacity-0 pointer-events-none absolute w-full'}`}>
          {autoAvailable && (
          <label className="flex items-center justify-between cursor-pointer rounded-xl border border-line bg-surface p-5 transition hover:border-accent/40">
            <div>
              <p className="text-sm font-semibold text-ink mb-1">{t('onboarding.autostart')}</p>
              <p className="text-xs text-muted pr-8">
                {t('onboarding.autostartDesc')}
              </p>
            </div>
            <div
              role="switch"
              aria-checked={auto}
              className={`relative h-6 w-11 shrink-0 border transition-colors ${
                auto ? 'border-accent bg-accent' : 'border-line bg-elevated'
              }`}
            >
              <span
                className={`absolute top-1/2 h-4 w-4 -translate-y-1/2 transition-all ${
                  auto ? 'left-6 bg-white' : 'left-1 bg-muted'
                }`}
              />
            </div>
            <input 
              type="checkbox" 
              className="hidden" 
              checked={auto} 
              onChange={(e) => setAuto(e.target.checked)} 
            />
          </label>
          )}

          <label className="flex items-center justify-between cursor-pointer rounded-xl border border-line bg-surface p-5 transition hover:border-accent/40">
            <div>
              <p className="text-sm font-semibold text-ink mb-1">{t('onboarding.tray')}</p>
              <p className="text-xs text-muted pr-8">
                {t('onboarding.trayDesc')}
              </p>
            </div>
            <div
              role="switch"
              aria-checked={tray}
              className={`relative h-6 w-11 shrink-0 border transition-colors ${
                tray ? 'border-accent bg-accent' : 'border-line bg-elevated'
              }`}
            >
              <span
                className={`absolute top-1/2 h-4 w-4 -translate-y-1/2 transition-all ${
                  tray ? 'left-6 bg-white' : 'left-1 bg-muted'
                }`}
              />
            </div>
            <input 
              type="checkbox" 
              className="hidden" 
              checked={tray} 
              onChange={(e) => setTray(e.target.checked)} 
            />
          </label>

          <label className="flex items-center justify-between cursor-pointer rounded-xl border border-line bg-surface p-5 transition hover:border-accent/40">
            <div>
              <p className="text-sm font-semibold text-ink mb-1">{t('onboarding.metrics')}</p>
              <p className="text-xs text-muted pr-8">
                {t('onboarding.metricsDesc')}
              </p>
            </div>
            <div
              role="switch"
              aria-checked={metrics}
              className={`relative h-6 w-11 shrink-0 border transition-colors ${
                metrics ? 'border-accent bg-accent' : 'border-line bg-elevated'
              }`}
            >
              <span
                className={`absolute top-1/2 h-4 w-4 -translate-y-1/2 transition-all ${
                  metrics ? 'left-6 bg-white' : 'left-1 bg-muted'
                }`}
              />
            </div>
            <input
              type="checkbox"
              className="hidden"
              checked={metrics}
              onChange={(e) => setMetrics(e.target.checked)}
            />
          </label>

          <label className="flex items-center justify-between cursor-pointer rounded-xl border border-line bg-surface p-5 transition hover:border-accent/40">
            <div>
              <p className="text-sm font-semibold text-ink mb-1">{t('onboarding.covers')}</p>
              <p className="text-xs text-muted pr-8">
                {t('onboarding.coversDesc')}
              </p>
            </div>
            <div
              role="switch"
              aria-checked={covers}
              className={`relative h-6 w-11 shrink-0 border transition-colors ${
                covers ? 'border-accent bg-accent' : 'border-line bg-elevated'
              }`}
            >
              <span
                className={`absolute top-1/2 h-4 w-4 -translate-y-1/2 transition-all ${
                  covers ? 'left-6 bg-white' : 'left-1 bg-muted'
                }`}
              />
            </div>
            <input
              type="checkbox"
              className="hidden"
              checked={covers}
              onChange={(e) => setCovers(e.target.checked)}
            />
          </label>

          <p className="col-span-2 text-center text-xs text-muted">
            {t('onboarding.scanDesc')}{' '}
            <button
              type="button"
              onClick={() => void openExternal(privacyUrl(i18n.language)).catch(() => {})}
              className="text-accent underline hover:text-ink"
            >
              {t('onboarding.privacy')}
            </button>
          </p>
        </div>

        {/* Steps dots */}
        <div className={`flex justify-center gap-2 mb-8 ${isLast ? 'mt-0' : 'mt-[168px]'}`}>
          {SLIDES.map((_, i) => (
            <button
              key={i}
              onClick={() => setStep(i)}
              className={`h-2 rounded-full transition-all duration-300 ${i === step ? 'w-8 bg-accent' : 'w-2 bg-line hover:bg-muted'}`}
            />
          ))}
        </div>

        {error && <p className="mb-4 text-center text-sm text-accent">{error}</p>}

        {/* Action Button */}
        <button
          onClick={handleNext}
          disabled={busy}
          className="mx-auto block w-full max-w-lg rounded-xl bg-accent py-4 text-base font-semibold text-primary-foreground transition hover:bg-accent-soft shadow-[0_0_20px_rgba(223,79,79,0.2)] disabled:opacity-50"
        >
          {busy ? t('onboarding.preparing') : isLast ? t('onboarding.scanLibrary') : t('onboarding.next')}
        </button>
      </div>
      </div>
    </div>
  );
}
