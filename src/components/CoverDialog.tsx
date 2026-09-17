'use client';

import { useRef, useState } from 'react';
import { useTranslation, Trans } from 'react-i18next';
import { setCover, setCoverImage } from '@/lib/tauri';
import { coverSrc } from '@/lib/cover';
import type { Game } from '@/lib/types';
import { CloseIcon, ImageIcon } from './icons';

/** Set or clear a manual cover for a single game. The cover can be a remote URL
 *  or a local image (dropped onto the zone, or picked from disk). Overrides win
 *  over the auto-resolved artwork, so a stubborn cover can always be fixed. */
export function CoverDialog({
  game,
  onClose,
  onSaved,
}: {
  game: Game;
  onClose: () => void;
  onSaved: (id: string, url: string | null) => void;
}) {
  // Prefill only manual (remote) overrides; a local cached path isn't editable text.
  const { t } = useTranslation();
  const isRemote = /^https?:\/\//i.test(game.cover_url ?? '');
  const [url, setUrl] = useState(isRemote ? (game.cover_url as string) : '');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  // What to show in the preview: the typed URL, else the game's current cover.
  const preview = coverSrc(url.trim() || game.cover_url);

  async function commit(value: string | null) {
    setBusy(true);
    setError(null);
    try {
      const stored = await setCover(game.id, value);
      onSaved(game.id, stored);
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  // Save a local image file (dropped or picked) as the cover.
  async function handleFile(file: File) {
    if (!file.type.startsWith('image/')) {
      setError(t('dialog.notImage'));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
      const ext = (file.name.split('.').pop() || file.type.split('/')[1] || 'jpg').toLowerCase();
      const path = await setCoverImage(game.id, bytes, ext);
      onSaved(game.id, path);
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-void/70 p-4 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="w-full max-w-md border border-line bg-surface p-6 shadow-card"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-5 flex items-center justify-between">
          <h2 className="line-clamp-1 font-display text-lg font-semibold text-ink">
            {t('dialog.coverTitle', { name: game.name })}
          </h2>
          <button
            onClick={onClose}
            className="grid h-8 w-8 shrink-0 place-items-center text-muted hover:bg-elevated hover:text-ink"
          >
            <CloseIcon className="h-5 w-5" />
          </button>
        </div>

        <div className="mb-4 flex gap-4">
          <div className="aspect-[2/3] w-24 shrink-0 overflow-hidden border border-line bg-elevated">
            {preview ? (
              // eslint-disable-next-line @next/next/no-img-element
              <img src={preview} alt="" className="h-full w-full object-cover" />
            ) : (
              <div className="grid h-full w-full place-items-center text-2xl font-bold text-accent/70">
                {game.name.charAt(0).toUpperCase()}
              </div>
            )}
          </div>
          <div className="min-w-0 flex-1">
            <label className="mb-1.5 block text-xs font-medium text-muted">
              {t('dialog.imageUrl')}
            </label>
            <input
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="https://…/cover.jpg"
              className="w-full border border-line bg-elevated px-3 py-2.5 text-sm text-ink outline-none placeholder:text-muted/60 focus:border-accent/60"
            />
            <p className="mt-2 text-xs leading-relaxed text-muted">{t('dialog.coverHelp')}</p>
          </div>
        </div>

        {/* Drop zone for a local image (also click to browse). */}
        <div
          onDragOver={(e) => {
            e.preventDefault();
            setDragOver(true);
          }}
          onDragLeave={() => setDragOver(false)}
          onDrop={(e) => {
            e.preventDefault();
            setDragOver(false);
            const file = e.dataTransfer.files?.[0];
            if (file) handleFile(file);
          }}
          onClick={() => inputRef.current?.click()}
          className={`mb-4 flex cursor-pointer flex-col items-center justify-center gap-2 border-2 border-dashed px-4 py-6 text-center transition ${
            dragOver
              ? 'border-accent bg-accent/10 text-ink'
              : 'border-line text-muted hover:border-accent/50 hover:text-ink'
          }`}
        >
          <ImageIcon className="h-6 w-6" />
          <span className="text-sm">
            <Trans i18nKey="dialog.dropImage" components={[<span key="0" className="text-ink underline" />]} />
          </span>
          <span className="text-xs text-muted/70">PNG, JPG, WEBP, GIF</span>
          <input
            ref={inputRef}
            type="file"
            accept="image/*"
            className="hidden"
            onChange={(e) => {
              const file = e.target.files?.[0];
              if (file) handleFile(file);
              e.target.value = '';
            }}
          />
        </div>

        {error && <p className="mb-4 text-sm text-accent">{error}</p>}

        <div className="flex justify-between gap-2">
          <button
            onClick={() => commit(null)}
            disabled={busy}
            className="px-3 py-2 text-sm text-muted hover:text-ink disabled:opacity-50"
          >
            {t('dialog.reset')}
          </button>
          <div className="flex gap-2">
            <button
              onClick={onClose}
              className="px-4 py-2 text-sm text-muted hover:text-ink"
            >
              {t('common.cancel')}
            </button>
            <button
              onClick={() => commit(url.trim() || null)}
              disabled={busy}
              className="bg-accent px-4 py-2 text-sm font-semibold text-white hover:bg-accent-soft disabled:opacity-50"
            >
              {busy ? t('dialog.saving') : t('common.save')}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
