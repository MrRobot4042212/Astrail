// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

// The launcher window: wires the library, its view state and its actions to the
// screen. Rules live in `lib/libraryView`, state in `hooks/`, markup in
// `components/`; what stays here is which dialog is open and who gets which prop.

import { useCallback, useEffect, useMemo, useState } from 'react';
import dynamic from 'next/dynamic';
import { onEvent } from '@/lib/events';
import { useTranslation } from 'react-i18next';
import { useLibrary } from '@/hooks/useLibrary';
import { useLibraryActions, type ConfirmRequest } from '@/hooks/useLibraryActions';
import { useLibraryView } from '@/hooks/useLibraryView';
import { useSelection } from '@/hooks/useSelection';
import { useSplashGate } from '@/hooks/useSplashGate';
import { useToast } from '@/hooks/useToast';
import { showMainWindow, getAppSettings } from '@/lib/tauri';
import type { Game, Category } from '@/lib/types';
import { Sidebar } from '@/components/Sidebar';
import { LibraryGrid } from '@/components/LibraryGrid';
import { ContextMenu, type MenuItem } from '@/components/ContextMenu';
import { categoryMenuItems, gameMenuItems } from '@/components/libraryMenus';
import { BulkActionBar } from '@/components/BulkActionBar';
import { Empty, SkeletonGrid, Toast } from '@/components/LibraryPlaceholders';
import { Splash } from '@/components/Splash';
import { IntroSplash } from '@/components/IntroSplash';
import { Footer } from '@/components/Footer';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { Home } from '@/components/Home';
import { TopBar } from '@/components/TopBar';
import { UpdatePrompt } from '@/components/UpdatePrompt';
import { useEscape } from '@/hooks/useEscape';

// Loaded on demand: none of these render until the user opens something, so
// keeping them in the first-load chunk only delayed the library appearing.
const BulkCategoryDialog = dynamic(() => import('@/components/BulkCategoryDialog').then((m) => m.BulkCategoryDialog), { ssr: false });
const AddAppDialog = dynamic(() => import('@/components/AddAppDialog').then((m) => m.AddAppDialog), { ssr: false });
const SettingsDialog = dynamic(() => import('@/components/SettingsDialog').then((m) => m.SettingsDialog), { ssr: false });
const HiddenGamesModal = dynamic(() => import('@/components/HiddenGamesModal').then((m) => m.HiddenGamesModal), { ssr: false });
const CoverDialog = dynamic(() => import('@/components/CoverDialog').then((m) => m.CoverDialog), { ssr: false });
const CategoryDialog = dynamic(() => import('@/components/CategoryDialog').then((m) => m.CategoryDialog), { ssr: false });
const NewCategoryDialog = dynamic(() => import('@/components/NewCategoryDialog').then((m) => m.NewCategoryDialog), { ssr: false });
const EditCategoryDialog = dynamic(() => import('@/components/EditCategoryDialog').then((m) => m.EditCategoryDialog), { ssr: false });
const Spotlight = dynamic(() => import('@/components/Spotlight').then((m) => m.Spotlight), { ssr: false });
const DetailView = dynamic(() => import('@/components/DetailView').then((m) => m.DetailView), { ssr: false });
const NotificationsPanel = dynamic(() => import('@/components/NotificationsPanel').then((m) => m.NotificationsPanel), { ssr: false });
const Onboarding = dynamic(() => import('@/components/Onboarding').then((m) => m.Onboarding), { ssr: false });
const GuidedTour = dynamic(() => import('@/components/GuidedTour').then((m) => m.GuidedTour), { ssr: false });

/**
 * Root: the launcher window.
 *
 * This document used to back both windows and pick a tree from the window label
 * at runtime, which meant the launcher bundle had to contain the overlay and the
 * overlay had to contain the launcher. The overlay now has its own route
 * (`src/app/overlay/page.tsx`), so each window loads only what it renders.
 */
export default function Root() {
  return <MainApp />;
}

function MainApp() {
  const { t } = useTranslation();
  // The window is created hidden (tauri.conf.json) and revealed here, after the
  // first paint, so users never see an empty white rectangle on startup.
  useEffect(() => {
    const raf = requestAnimationFrame(() => {
      showMainWindow().catch(() => {});
    });
    return () => cancelAnimationFrame(raf);
  }, []);
  const [introDone, setIntroDone] = useState(false);

  // Onboarding gate: 'unknown' until settings load, then 'needed' (first run) or
  // 'done'. The library only auto-scans once we're past onboarding, so the slow
  // native scan never freezes the onboarding screen — the user kicks it off with
  // the "Escanear" button, which then shows the splash.
  const [onboardingState, setOnboardingState] = useState<'unknown' | 'needed' | 'done'>(
    'unknown',
  );
  const needsOnboarding = onboardingState === 'needed';
  const autoScan = onboardingState === 'done';

  const {
    games,
    loading,
    error,
    refresh,
    resetArt,
    setGames,
    categoryMeta,
    refreshCategories,
    booting,
    coverProgress,
    playtimes,
  } = useLibrary(autoScan);
  const view = useLibraryView(games, categoryMeta, playtimes);
  const { filter, setFilter, query, setQuery, categoryNames, visible } = view;
  const selection = useSelection();
  const { toast, flash } = useToast();
  // Mounted while the first scan runs; fades into the main screen when it
  // finishes or the user skips it.
  const splash = useSplashGate(booting && !needsOnboarding);

  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  const [showBulkCats, setShowBulkCats] = useState(false);
  const [spotlight, setSpotlight] = useState(false);
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null);
  const [showAdd, setShowAdd] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [showHiddenGames, setShowHiddenGames] = useState(false);
  const [editingCover, setEditingCover] = useState<Game | null>(null);
  const [editingCategories, setEditingCategories] = useState<Game | null>(null);
  const [showNewCategory, setShowNewCategory] = useState(false);
  const [editingCategory, setEditingCategory] = useState<Category | null>(null);
  const [dragging, setDragging] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [showNotifications, setShowNotifications] = useState(false);
  // Guided product tour: shown once after the first scan, re-launchable from Ajustes.
  const [showTour, setShowTour] = useState(false);
  // True after onboarding finishes on a fresh install, so the tour fires as soon
  // as the library has loaded (we wait for at least one game to anchor the steps).
  const [tourPending, setTourPending] = useState(false);

  const actions = useLibraryActions({
    games,
    setGames,
    refresh,
    refreshCategories,
    flash,
    ask: setConfirm,
    selectedIds: selection.ids,
    exitSelection: selection.exit,
    filter,
    setFilter,
    setSelectedId,
  });
  const { launch, openFolder, toggleFavorite, toggleType, remove, hide } = actions;

  useEffect(() => {
    getAppSettings()
      // Fail open: if settings can't load, don't trap the user in onboarding —
      // treat it as done so the library still scans.
      .then((s) => setOnboardingState(s.setup_completed ? 'done' : 'needed'))
      .catch(() => setOnboardingState('done'));
  }, []);

  // Fire the guided tour after the first scan finishes: wait until the splash is
  // gone and the library has settled, so the steps can anchor to a real card.
  useEffect(() => {
    if (tourPending && !booting && !splash.mounted) {
      setTourPending(false);
      setShowTour(true);
    }
  }, [tourPending, booting, splash.mounted]);

  // The game whose detail page is open (kept fresh from `games` by id).
  const selected = useMemo(
    () => (selectedId ? games.find((g) => g.id === selectedId) ?? null : null),
    [games, selectedId],
  );

  // Global Spotlight: the Rust global shortcut emits this when triggered.
  useEffect(() => {
    const un = onEvent('open-spotlight', () => setSpotlight(true));
    return () => {
      un.then((f) => f());
    };
  }, []);

  // Esc closes the detail page, or exits multi-select.
  const exitSelection = selection.exit;
  useEscape(() => {
    if (selected) setSelectedId(null);
    else exitSelection();
  }, Boolean(selected) || selection.active);

  // Stable identity: both are handed to memoized `GameCard`s. Every action the
  // menu closes over is stable too, so this only changes with the language.
  const handleCardContextMenu = useCallback(
    (g: Game, x: number, y: number) => {
      const items = gameMenuItems(g, t, {
        launch,
        toggleFavorite,
        toggleType,
        editCategories: setEditingCategories,
        editCover: setEditingCover,
        openFolder,
        remove,
        hide,
      });
      setMenu({ x, y, items });
    },
    [t, launch, toggleFavorite, toggleType, openFolder, remove, hide],
  );
  const handleOpen = useCallback((g: Game) => setSelectedId(g.id), []);

  // Re-scan from scratch, showing the splash again (reset any earlier skip).
  function handleRescan() {
    splash.rearm();
    refresh(true);
  }

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-void text-ink">
      {!introDone && <IntroSplash onFinish={() => setIntroDone(true)} />}

      {introDone && needsOnboarding && (
        <Onboarding
          onComplete={() => {
            setOnboardingState('done');
            // Queue the guided tour; it starts once the first scan has loaded.
            setTourPending(true);
          }}
        />
      )}

      {introDone && splash.mounted && (
        <Splash progress={coverProgress} exiting={splash.exiting} onSkip={splash.skip} />
      )}

      {showTour && (
        <GuidedTour
          onFinish={() => setShowTour(false)}
          setView={(f) => {
            setQuery('');
            setFilter(f);
          }}
          resetUi={() => {
            setMenu(null);
            setSelectedId(null);
            selection.exit();
          }}
        />
      )}

      <div className="flex min-h-0 flex-1">
        <Sidebar
          filter={filter}
          onFilter={(f) => {
            setFilter(f);
            setSelectedId(null);
          }}
          counts={view.counts}
          categories={view.categories}
          onAddCategory={() => setShowNewCategory(true)}
          onDropGame={actions.dropOnSidebar}
          isDragging={dragging}
          onOpenSettings={() => setShowSettings(true)}
          onOpenHidden={() => setShowHiddenGames(true)}
          onCategoryContextMenu={(cat, x, y) =>
            setMenu({
              x,
              y,
              items: categoryMenuItems(cat, t, { edit: setEditingCategory, remove: actions.deleteCategory }),
            })
          }
          onReorderCategories={actions.reorderCategories}
        />

        <main className="flex min-w-0 flex-1 flex-col">
          <ErrorBoundary scope="view" resetKey={selectedId ?? filter}>
          {selected ? (
            <DetailView
              game={selected}
              onBack={() => setSelectedId(null)}
              onLaunch={launch}
              onToggleFavorite={toggleFavorite}
              onToggleType={toggleType}
              onEditCover={setEditingCover}
              onEditCategories={setEditingCategories}
              onRemove={selected.source === 'manual' ? remove : undefined}
              onHide={selected.source !== 'manual' ? hide : undefined}
            />
          ) : (
            <>
              {/* Top bar */}
              <TopBar
                query={query}
                setQuery={(q) => {
                  setQuery(q);
                  setSelectedId(null);
                }}
                showingHome={view.showingHome}
                sort={view.sort}
                setSort={view.setSort}
                handleRescan={handleRescan}
                loading={loading}
                setShowNotifications={setShowNotifications}
                setShowAdd={setShowAdd}
                onStartTour={() => setShowTour(true)}
              />

              {/* Content */}
              <section className="min-h-0 flex-1 overflow-y-auto px-6 py-6">
                {view.showingHome ? (
                  <div data-tour="home">
                    <Home games={games} playtimes={playtimes} onOpen={handleOpen} onLaunch={launch} />
                  </div>
                ) : loading && games.length === 0 ? (
                  <SkeletonGrid />
                ) : error && games.length === 0 ? (
                  <Empty
                    title={t('library.loadError')}
                    body={error}
                    action={{ label: t('common.retry'), onClick: handleRescan }}
                  />
                ) : visible.length === 0 ? (
                  <Empty
                    title={query ? t('library.noResults') : t('library.empty')}
                    body={query ? t('library.noResultsBody') : t('library.emptyBody')}
                    action={query ? undefined : { label: t('library.addApp'), onClick: () => setShowAdd(true) }}
                  />
                ) : (
                  <LibraryGrid
                    games={visible}
                    selectionMode={selection.active}
                    selectedIds={selection.ids}
                    onLaunch={launch}
                    onRemove={remove}
                    onHide={hide}
                    onEditCover={setEditingCover}
                    onToggleFavorite={toggleFavorite}
                    onEditCategories={setEditingCategories}
                    onDragStateChange={setDragging}
                    onOpen={handleOpen}
                    onContextMenu={handleCardContextMenu}
                    onToggleSelect={selection.toggle}
                  />
                )}
              </section>
            </>
          )}
          </ErrorBoundary>
          <Footer />
        </main>
      </div>

      <UpdatePrompt />

      {showNotifications && (
        <NotificationsPanel onClose={() => setShowNotifications(false)} />
      )}

      {showAdd && (
        <AddAppDialog
          onClose={() => setShowAdd(false)}
          onAdded={(g) => {
            setGames((prev) =>
              [...prev, g].sort((a, b) => a.name.localeCompare(b.name)),
            );
            flash(t('toast.added', { name: g.name }));
          }}
        />
      )}

      {showSettings && (
        <SettingsDialog
          onClose={() => setShowSettings(false)}
          onChanged={() => {
            flash(t('toast.updatingLibrary'));
            // Wiping the cover cache means the art has to be asked for again;
            // without this the session's "already resolved" set makes the next
            // pass skip every single entry.
            resetArt();
            refresh();
          }}
          onStartTour={() => {
            setShowSettings(false);
            setShowTour(true);
          }}
        />
      )}

      {showHiddenGames && (
        <HiddenGamesModal
          onClose={() => setShowHiddenGames(false)}
          onChanged={() => {
            flash(t('toast.libraryUpdated'));
            // Restored games were skipped by earlier cover passes.
            resetArt();
            refresh(false);
          }}
        />
      )}

      {editingCover && (
        <CoverDialog
          game={editingCover}
          onClose={() => setEditingCover(null)}
          onSaved={(id, url) => {
            setGames((prev) =>
              prev.map((g) => (g.id === id ? { ...g, cover_url: url } : g)),
            );
            flash(url ? t('toast.coverUpdated') : t('toast.coverReset'));
          }}
        />
      )}

      {editingCategories && (
        <CategoryDialog
          game={editingCategories}
          allCategories={categoryNames}
          onClose={() => setEditingCategories(null)}
          onSaved={(id, cats) => {
            setGames((prev) =>
              prev.map((g) => (g.id === id ? { ...g, categories: cats } : g)),
            );
            flash(t('toast.categoriesUpdated'));
          }}
        />
      )}

      {editingCategory && (
        <EditCategoryDialog
          category={editingCategory}
          existing={categoryNames}
          onClose={() => setEditingCategory(null)}
          onSaved={async () => {
            const old = editingCategory.name;
            await refreshCategories();
            refresh();
            // If the active filter pointed at the renamed/merged category, it may
            // no longer exist by that exact name; fall back to "Todo".
            if (filter === `cat:${old}`) setFilter('all');
            flash(t('toast.categoryUpdated'));
          }}
        />
      )}

      {showNewCategory && (
        <NewCategoryDialog
          existing={categoryNames}
          onClose={() => setShowNewCategory(false)}
          onCreated={async (name) => {
            // Await the reload so `categories` includes the new name before we
            // switch the filter — otherwise the empty-filter guard would bounce
            // us back to "Todo" on the next render.
            await refreshCategories();
            setFilter(`cat:${name}`);
            flash(t('toast.categoryCreated', { name }));
          }}
        />
      )}

      {selection.active && selection.ids.size > 0 && (
        <BulkActionBar
          count={selection.ids.size}
          onSelectAll={() => selection.selectAll(visible)}
          onFavorite={() => actions.bulkFavorite(true)}
          onCategories={() => setShowBulkCats(true)}
          onHide={actions.bulkHide}
          onCancel={selection.exit}
        />
      )}

      {showBulkCats && (
        <BulkCategoryDialog
          count={selection.ids.size}
          allCategories={categoryNames}
          onClose={() => setShowBulkCats(false)}
          onApply={async (cats) => {
            await actions.bulkAddCategories(cats);
            selection.exit();
          }}
        />
      )}

      {spotlight && (
        <Spotlight games={games} onLaunch={launch} onClose={() => setSpotlight(false)} />
      )}

      {menu && (
        <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />
      )}

      {confirm && (
        <ConfirmDialog
          title={confirm.title}
          message={confirm.message}
          confirmLabel={confirm.confirmLabel}
          onConfirm={confirm.onConfirm}
          onClose={() => setConfirm(null)}
        />
      )}

      <Toast message={toast} />
    </div>
  );
}
