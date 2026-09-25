// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { onEvent } from '@/lib/events';
import { Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { abortUpdate, checkUpdate, prepareForUpdate } from '@/lib/tauri';
import type { UpdateChannel } from '@/lib/types';
import { RefreshIcon, CloseIcon } from './icons';

type Phase = 'idle' | 'available' | 'downloading' | 'ready' | 'error';

/** A launcher that lives in the tray can stay up for days: checking only at startup
 *  left it on an old version until the next reboot. */
const RECHECK_MS = 6 * 60 * 60 * 1000;

/** Checks for a newer signed build on startup, every few hours and when the update
 *  channel changes, and offers a one-click update (download → install → relaunch).
 *  The check runs in Rust, which picks the endpoints of the configured channel.
 *  Silent if up to date, offline, or running in dev (the check just fails and is
 *  ignored). */
export function UpdatePrompt() {
  const { t } = useTranslation();
  const [update, setUpdate] = useState<Update | null>(null);
  const [phase, setPhase] = useState<Phase>('idle');
  const [pct, setPct] = useState(0);
  const [dismissed, setDismissed] = useState(false);
  const [channel, setChannel] = useState<UpdateChannel>('stable');
  // Read by the re-check, which outlives the render it was created in.
  const phaseRef = useRef<Phase>('idle');
  const offeredRef = useRef<string | null>(null);

  useEffect(() => {
    phaseRef.current = phase;
  }, [phase]);

  useEffect(() => {
    let cancelled = false;
    const run = async () => {
      // Never swap out an update that is being downloaded or installed.
      if (phaseRef.current === 'downloading' || phaseRef.current === 'ready') return;
      try {
        const offer = await checkUpdate();
        if (cancelled) return;
        if (offer) {
          // A version the user already dismissed stays dismissed.
          if (offeredRef.current !== offer.version) setDismissed(false);
          offeredRef.current = offer.version;
          setUpdate(new Update(offer));
          setChannel(offer.channel);
          setPhase('available');
        } else if (phaseRef.current === 'available') {
          // Left the beta channel: that offer no longer applies.
          offeredRef.current = null;
          setUpdate(null);
          setPhase('idle');
        }
      } catch {
        // No release yet, offline, or dev build: nothing to offer.
      }
    };
    run();
    const timer = window.setInterval(run, RECHECK_MS);
    const unlisten = onEvent('update-channel-changed', run);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
      unlisten.then((f) => f());
    };
  }, []);

  async function install() {
    if (!update) return;
    setPhase('downloading');
    setPct(0);
    let total = 0;
    let got = 0;
    let prepared = false;
    try {
      await update.download((event) => {
        if (event.event === 'Started') {
          total = event.data.contentLength ?? 0;
        } else if (event.event === 'Progress') {
          got += event.data.chunkLength;
          if (total > 0) setPct(Math.round((got / total) * 100));
        } else if (event.event === 'Finished') {
          setPct(100);
        }
      });
      // Install exits the process without the normal shutdown, so stop the
      // sidecars (kernel driver, ETW session) first. Only after the download: a
      // failed download must not cost the running game its metrics.
      // Set before the call: it suspends the sidecars first, so even a rejection
      // needs the abort below.
      prepared = true;
      await prepareForUpdate();
      await update.install();
      setPhase('ready');
      // Restart into the freshly installed version.
      await relaunch();
    } catch {
      if (prepared) await abortUpdate().catch(() => {});
      setPhase('error');
    }
  }

  if (phase === 'idle' || dismissed) return null;

  return (
    <div className="fixed bottom-14 right-4 z-50 w-80 border border-line bg-elevated p-4 shadow-card">
      <div className="mb-2 flex items-center gap-2">
        <RefreshIcon className="h-[18px] w-[18px] text-accent" />
        <p className="flex-1 text-sm font-semibold text-ink">
          {phase === 'error' ? t('update.errorTitle') : t('update.newVersion', { version: update?.version ?? '' })}
        </p>
        {(phase === 'available' || phase === 'error') && (
          <button
            onClick={() => setDismissed(true)}
            className="grid h-6 w-6 place-items-center text-muted transition hover:text-ink"
          >
            <CloseIcon className="h-4 w-4" />
          </button>
        )}
      </div>

      {phase === 'available' && (
        <>
          {channel === 'beta' && (
            <p className="mb-2 text-xs font-medium text-accent">{t('update.betaTag')}</p>
          )}
          <p className="mb-3 text-xs leading-relaxed text-muted">
            {t('update.body')}
          </p>
          <button
            onClick={install}
            className="w-full bg-accent py-2 text-sm font-semibold text-primary-foreground transition hover:bg-accent-soft"
          >
            {t('update.updateRestart')}
          </button>
        </>
      )}

      {phase === 'downloading' && (
        <>
          <p className="mb-2 text-xs text-muted">{t('update.downloading', { pct })}</p>
          <div className="h-1.5 w-full bg-surface">
            <div className="h-full bg-accent transition-all" style={{ width: `${pct}%` }} />
          </div>
        </>
      )}

      {phase === 'ready' && <p className="text-xs text-muted">{t('update.restarting')}</p>}

      {phase === 'error' && (
        <p className="text-xs leading-relaxed text-muted">
          {t('update.errorBody')}
        </p>
      )}
    </div>
  );
}
