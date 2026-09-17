// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useEffect, useMemo, useState, useCallback, useRef } from 'react';
import dynamic from 'next/dynamic';
import { listen } from '@tauri-apps/api/event';
import { useLibrary } from '@/hooks/useLibrary';
import {
  launchGame,
  showMainWindow,
  removeGame,
  hideGame,
  setFavorite,
  setCategories,
  openGameFolder,
  removeCategory,
  setCategoryOrder,
  setGameType,
  getAppSettings,
} from '@/lib/tauri';
import type { Game, Category } from '@/lib/types';
import { Sidebar, type Filter } from '@/components/Sidebar';
import { LibraryGrid } from '@/components/LibraryGrid';
import { ContextMenu, type MenuItem } from '@/components/ContextMenu';
import { Splash } from '@/components/Splash';
import { IntroSplash } from '@/components/IntroSplash';
import { Footer } from '@/components/Footer';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { Home } from '@/components/Home';
import { TopBar, type SortKey } from '@/components/TopBar';
import { UpdatePrompt } from '@/components/UpdatePrompt';
import { useTranslation } from 'react-i18next';
import { SOURCE_ORDER } from '@/lib/sources';
import { fuzzyScore } from '@/lib/fuzzy';
import {
  PlayIcon,
  StarIcon,
  TagIcon,
  ImageIcon,
  EyeOffIcon,
  TrashIcon,
  FolderIcon,
  PencilIcon,
  AppIcon,
  GridIcon,
} from '@/components/icons';

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

/** Folder to reveal for a game: its install dir, else the exe's parent. */
function folderOf(game: Game): string | null {
  if (game.install_dir) return game.install_dir;
  const exe = game.executable;
  if (!exe) return null;
  const i = Math.max(exe.lastIndexOf('\\'), exe.lastIndexOf('/'));
  return i > 0 ? exe.slice(0, i) : null;
}

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
  const [splashDone, setSplashDone] = useState(false);
  // Splash visibility with a fade-out: kept mounted briefly after it should hide so
  // it can fade into the main screen instead of popping away.
  const [splashMounted, setSplashMounted] = useState(false);
  const [splashExiting, setSplashExiting] = useState(false);
  const [filter, setFilter] = useState<Filter>('home');
  const [query, setQuery] = useState('');
  // Debounced query for the expensive fuzzy-filter (visible useMemo): the input
  // always reflects the raw query immediately, but the grid only re-filters
  // 150 ms after the user stops typing — eliminating per-keystroke recalculations
  // over large libraries.
  const [debouncedQuery, setDebouncedQuery] = useState('');
  const [sort, setSort] = useState<SortKey>('name');
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  const [selectMode, setSelectMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [showBulkCats, setShowBulkCats] = useState(false);
  const [spotlight, setSpotlight] = useState(false);
  const [confirm, setConfirm] = useState<{
    title: string;
    message: React.ReactNode;
    confirmLabel: string;
    onConfirm: () => void;
  } | null>(null);
  const [showAdd, setShowAdd] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [showHiddenGames, setShowHiddenGames] = useState(false);
  const [editingCover, setEditingCover] = useState<Game | null>(null);
  const [editingCategories, setEditingCategories] = useState<Game | null>(null);
  const [showNewCategory, setShowNewCategory] = useState(false);
  const [editingCategory, setEditingCategory] = useState<Category | null>(null);
  const [dragging, setDragging] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [showNotifications, setShowNotifications] = useState(false);
  // Guided product tour: shown once after the first scan, re-launchable from Ajustes.
  const [showTour, setShowTour] = useState(false);
  // True after onboarding finishes on a fresh install, so the tour fires as soon
  // as the library has loaded (we wait for at least one game to anchor the steps).
  const [tourPending, setTourPending] = useState(false);

  useEffect(() => {
    getAppSettings()
      // Fail open: if settings can't load, don't trap the user in onboarding —
      // treat it as done so the library still scans.
      .then((s) => setOnboardingState(s.setup_completed ? 'done' : 'needed'))
      .catch(() => setOnboardingState('done'));
  }, []);

  // Drive the splash mount + fade-out. Mount while the first scan is running; when
  // it finishes (or the user skips), fade out for 500ms, then unmount — revealing
  // the main screen underneath.
  const splashActive = booting && !needsOnboarding && !splashDone;
  useEffect(() => {
    if (splashActive) {
      setSplashMounted(true);
      setSplashExiting(false);
    } else if (splashMounted) {
      setSplashExiting(true);
      const t = window.setTimeout(() => setSplashMounted(false), 500);
      return () => window.clearTimeout(t);
    }
  }, [splashActive, splashMounted]);

  // Fire the guided tour after the first scan finishes: wait until the splash is
  // gone and the library has settled, so the steps can anchor to a real card.
  useEffect(() => {
    if (tourPending && !booting && !splashMounted) {
      setTourPending(false);
      setShowTour(true);
    }
  }, [tourPending, booting, splashMounted]);

  // The game whose detail page is open (kept fresh from `games` by id).
  const selected = useMemo(
    () => (selectedId ? games.find((g) => g.id === selectedId) ?? null : null),
    [games, selectedId],
  );

  // The dashboard replaces the grid on the "Inicio" filter (unless searching).
  const showingHome = filter === 'home' && !query.trim();

  // Categories shown in the sidebar: explicitly-created ones first, in their saved
  // order (with icons, persist even when empty), then any in-use-only ones
  // appended alphabetically. The explicit entry wins so its icon/order are kept.
  const categories = useMemo(() => {
    const result: Category[] = [];
    const seen = new Set<string>();
    for (const c of categoryMeta) {
      const key = c.name.toLowerCase();
      if (!seen.has(key)) {
        result.push(c);
        seen.add(key);
      }
    }
    const inUse = new Set<string>();
    for (const g of games) for (const c of g.categories ?? []) inUse.add(c);
    const extra = [...inUse]
      .filter((c) => !seen.has(c.toLowerCase()))
      .sort((a, b) => a.localeCompare(b));
    for (const name of extra) result.push({ name, icon: null });
    return result;
  }, [games, categoryMeta]);

  const categoryNames = useMemo(() => categories.map((c) => c.name), [categories]);

  // Debounce: update debouncedQuery 150ms after the user stops typing.
  useEffect(() => {
    const t = setTimeout(() => setDebouncedQuery(query), 150);
    return () => clearTimeout(t);
  }, [query]);

  const counts = useMemo<Record<string, number>>(() => {
    // Single O(N) pass over games instead of N separate Array.filter() calls
    // (one per source + one per category). For 500 games + 10 categories the
    // old approach did ~6500 iterations; this does exactly 500.
    const base: Record<string, number> = { all: 0, favorites: 0 };
    for (const source of SOURCE_ORDER) base[source] = 0;
    for (const name of categoryNames) base[`cat:${name}`] = 0;
    for (const g of games) {
      base.all++;
      if (g.favorite) base.favorites++;
      base[g.source] = (base[g.source] ?? 0) + 1;
      for (const c of g.categories ?? []) {
        const key = `cat:${c}`;
        if (key in base) base[key]++;
      }
    }
    return base;
  }, [games, categoryNames]);

  const visible = useMemo(() => {
    const inFilter = games.filter((g) => {
      // Home has no own grid; when a query is typed there we search everything.
      if (filter === 'all' || filter === 'home') return true;
      if (filter === 'favorites') return !!g.favorite;
      if (filter.startsWith('cat:')) return g.categories?.includes(filter.slice(4)) ?? false;
      return g.source === filter;
    });

    // A query takes over ordering: fuzzy-match and rank by score.
    // Uses the debounced query so the grid re-filters only after typing pauses.
    const q = debouncedQuery.trim();
    if (q) {
      return inFilter
        .map((g) => ({ g, s: fuzzyScore(q, g.name) }))
        .filter((x): x is { g: Game; s: number } => x.s !== null)
        .sort((a, b) => b.s - a.s)
        .map((x) => x.g);
    }

    // Otherwise apply the chosen sort (ties broken by name).
    const secs = (id: string) => playtimes[id]?.seconds ?? 0;
    const last = (id: string) => playtimes[id]?.last_played ?? 0;
    const byName = (a: Game, b: Game) => a.name.localeCompare(b.name);
    const arr = [...inFilter];
    if (sort === 'played') arr.sort((a, b) => secs(b.id) - secs(a.id) || byName(a, b));
    else if (sort === 'recent') arr.sort((a, b) => last(b.id) - last(a.id) || byName(a, b));
    else arr.sort(byName);
    return arr;
  }, [games, filter, debouncedQuery, sort, playtimes]);

  // Items for the right-click context menu on a card.
  const menuItems = (game: Game): MenuItem[] => menuItemsImpl(game);
  function menuItemsImpl(game: Game): MenuItem[] {
    const items: MenuItem[] = [
      { label: t('menu.play'), icon: <PlayIcon className="h-4 w-4" />, onClick: () => handleLaunch(game) },
      {
        label: game.favorite ? t('menu.removeFavorite') : t('menu.addFavorite'),
        icon: <StarIcon className="h-4 w-4" fill={game.favorite ? 'currentColor' : 'none'} />,
        onClick: () => handleToggleFavorite(game),
      },
      { label: t('menu.categories'), icon: <TagIcon className="h-4 w-4" />, onClick: () => setEditingCategories(game) },
      { label: t('menu.changeCover'), icon: <ImageIcon className="h-4 w-4" />, onClick: () => setEditingCover(game) },
      game.source === 'app'
        ? { label: t('menu.markAsGame'), icon: <GridIcon className="h-4 w-4" />, onClick: () => handleToggleType(game) }
        : { label: t('menu.markAsApp'), icon: <AppIcon className="h-4 w-4" />, onClick: () => handleToggleType(game) },
    ];
    const folder = folderOf(game);
    if (folder) {
      items.push({
        label: t('menu.openFolder'),
        icon: <FolderIcon className="h-4 w-4" />,
        onClick: () => {
          openGameFolder(game.id).catch(() => flash(t('toast.folderOpenFailed')));
        },
      });
    }
    items.push({ type: 'separator' });
    if (game.source === 'manual') {
      items.push({ label: t('menu.remove'), danger: true, icon: <TrashIcon className="h-4 w-4" />, onClick: () => handleRemove(game) });
    } else {
      items.push({ label: t('menu.hide'), danger: true, icon: <EyeOffIcon className="h-4 w-4" />, onClick: () => handleHide(game) });
    }
    return items;
  }

  // Items for the right-click context menu on a custom category.
  function categoryMenuItems(cat: Category): MenuItem[] {
    return [
      {
        label: t('menu.edit'),
        icon: <PencilIcon className="h-4 w-4" />,
        onClick: () => setEditingCategory(cat),
      },
      { type: 'separator' },
      {
        label: t('menu.deleteCategory'),
        danger: true,
        icon: <TrashIcon className="h-4 w-4" />,
        onClick: () => handleDeleteCategory(cat),
      },
    ];
  }

  // If the active filter disappears from the sidebar (its last game left the
  // favorites/category), fall back to "Todo" instead of an empty, hidden view.
  useEffect(() => {
    if (filter.startsWith('cat:') && !categoryNames.includes(filter.slice(4))) {
      setFilter('all');
    }
  }, [filter, categoryNames]);

  // Global Spotlight: the Rust global shortcut emits this when triggered.
  useEffect(() => {
    const un = listen('open-spotlight', () => setSpotlight(true));
    return () => {
      un.then((f) => f());
    };
  }, []);

  // Esc closes the detail page, or exits multi-select.
  useEffect(() => {
    if (!selected && !selectMode) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      if (selected) setSelectedId(null);
      else if (selectMode) exitSelection();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [selected, selectMode]);

  // The menu builder closes over most of MainApp's state, so it cannot be
  // memoized — but the *callback* handed to each card can be, by reading the
  // latest builder through a ref. Same for opening the detail page.
  const menuItemsRef = useRef(menuItems);
  useEffect(() => {
    menuItemsRef.current = menuItems;
  });
  const handleCardContextMenu = useCallback((g: Game, x: number, y: number) => {
    setMenu({ x, y, items: menuItemsRef.current(g) });
  }, []);
  const handleOpen = useCallback((g: Game) => setSelectedId(g.id), []);

  const toastTimer = useRef<number | null>(null);
  const flash = useCallback((msg: string) => {
    setToast(msg);
    if (toastTimer.current !== null) window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => {
      toastTimer.current = null;
      setToast(null);
    }, 2500);
  }, []);
  useEffect(
    () => () => {
      if (toastTimer.current !== null) window.clearTimeout(toastTimer.current);
    },
    [],
  );

  // Notificación de mando conectado
  useEffect(() => {
    const onConnect = (e: GamepadEvent) => {
      flash(t('toast.gamepadConnected', { id: e.gamepad.id }));
    };
    window.addEventListener('gamepadconnected', onConnect);
    return () => window.removeEventListener('gamepadconnected', onConnect);
  }, [flash, t]);

  // Re-scan from scratch, showing the splash again (reset any earlier skip).
  function handleRescan() {
    setSplashDone(false);
    refresh(true);
  }

  // Stable identity: this is passed to memoized `GameCard`s.
  const handleLaunch = useCallback(async (game: Game) => {
    try {
      await launchGame(game.id);
      flash(t('toast.launching', { name: game.name }));
    } catch (e) {
      flash(t('toast.launchFailed', { error: String(e) }));
    }
  }, [t, flash]);

  // --- Category management --------------------------------------------------
  async function handleReorderCategories(names: string[]) {
    try {
      await setCategoryOrder(names);
      await refreshCategories();
    } catch {
      flash(t('toast.reorderCategoriesFailed'));
    }
  }

  function handleDeleteCategory(cat: Category) {
    setConfirm({
      title: t('confirm.deleteCategoryTitle'),
      message: t('confirm.deleteCategoryBody', { name: cat.name }),
      confirmLabel: t('common.delete'),
      onConfirm: () => doDeleteCategory(cat),
    });
  }

  async function doDeleteCategory(cat: Category) {
    try {
      await removeCategory(cat.name);
      if (filter === `cat:${cat.name}`) setFilter('all');
      await refreshCategories();
      refresh();
      flash(t('toast.categoryDeleted', { name: cat.name }));
    } catch {
      flash(t('toast.categoryDeleteFailed'));
    }
  }

  // --- Multi-select ---------------------------------------------------------
  // Stable identity: this is passed to memoized `GameCard`s.
  const toggleSelect = useCallback((game: Game) => {
    setSelectMode(true);
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(game.id)) next.delete(game.id);
      else next.add(game.id);
      return next;
    });
  }, []);

  function exitSelection() {
    setSelectMode(false);
    setSelectedIds(new Set());
  }

  async function bulkFavorite(value: boolean) {
    const ids = [...selectedIds];
    setGames((prev) =>
      prev.map((g) => (selectedIds.has(g.id) ? { ...g, favorite: value } : g)),
    );
    await Promise.allSettled(ids.map((id) => setFavorite(id, value)));
    flash(value ? t('toast.favoritedCount', { count: ids.length }) : t('toast.unfavoritedCount', { count: ids.length }));
  }

  function bulkHide() {
    const count = selectedIds.size;
    setConfirm({
      title: t('confirm.hideSelectionTitle'),
      message: t('confirm.hideSelectionBody', { count }),
      confirmLabel: t('common.hide'),
      onConfirm: doBulkHide,
    });
  }

  async function doBulkHide() {
    const ids = [...selectedIds];
    setGames((prev) => prev.filter((g) => !selectedIds.has(g.id)));
    exitSelection();
    const res = await Promise.allSettled(ids.map((id) => hideGame(id)));
    if (res.some((r) => r.status === 'rejected')) refresh();
    flash(t('toast.hiddenCount', { count: ids.length }));
  }

  async function bulkAddCategories(cats: string[]) {
    const ids = [...selectedIds];
    setGames((prev) =>
      prev.map((g) => {
        if (!selectedIds.has(g.id)) return g;
        const current = g.categories ?? [];
        const merged = [...current];
        for (const c of cats) {
          if (!merged.some((x) => x.toLowerCase() === c.toLowerCase())) merged.push(c);
        }
        return { ...g, categories: merged };
      }),
    );
    const games_ = games.filter((g) => selectedIds.has(g.id));
    await Promise.allSettled(
      games_.map((g) => {
        const current = g.categories ?? [];
        const merged = [...current];
        for (const c of cats) {
          if (!merged.some((x) => x.toLowerCase() === c.toLowerCase())) merged.push(c);
        }
        return setCategories(g.id, merged);
      }),
    );
    flash(t('toast.categoriesAddedTo', { count: ids.length }));
  }

  const doRemove = useCallback(
    async (game: Game) => {
      // Functional setters only: this is captured by a memoized handler, so
      // reading render state from the closure would pin it to one render.
      setSelectedId((cur) => (cur === game.id ? null : cur));
      setGames((prev) => prev.filter((g) => g.id !== game.id));
      try {
        await removeGame(game.id);
      } catch {
        refresh();
      }
    },
    [refresh, setGames],
  );

  const doHide = useCallback(
    async (game: Game) => {
      setSelectedId((cur) => (cur === game.id ? null : cur));
      setGames((prev) => prev.filter((g) => g.id !== game.id));
      try {
        await hideGame(game.id);
      } catch {
        refresh();
      }
    },
    [refresh, setGames],
  );

  // Stable identity: this is passed to memoized `GameCard`s.
  const handleRemove = useCallback((game: Game) => {
    setConfirm({
      title: t('confirm.removeTitle'),
      message: t('confirm.removeBody', { name: game.name }),
      confirmLabel: t('common.remove'),
      onConfirm: () => doRemove(game),
    });
  }, [t, doRemove]);


  // Stable identity: this is passed to memoized `GameCard`s.
  const handleHide = useCallback((game: Game) => {
    setConfirm({
      title: t('confirm.hideTitle'),
      message: t('confirm.hideBody', { name: game.name }),
      confirmLabel: t('common.hide'),
      onConfirm: () => doHide(game),
    });
  }, [t, doHide]);


  // Reclassify an entry between game and application. The backend re-derives the
  // real source on each scan, so we optimistically mirror its mapping (→app sets
  // 'app'; →game from an app becomes the generic 'windows' game source) and then
  // refresh so covers/grouping reconcile (a now-game gets IGDB art).
  // Stable identity: this is passed to memoized `GameCard`s.
  const handleToggleType = useCallback(async (game: Game) => {
    const toApp = game.source !== 'app';
    const optimisticSource = toApp ? 'app' : 'windows';
    setGames((prev) =>
      prev.map((g) =>
        g.id === game.id ? { ...g, source: optimisticSource, icon: undefined } : g,
      ),
    );
    flash(toApp ? t('toast.toApp', { name: game.name }) : t('toast.toGame', { name: game.name }));
    try {
      await setGameType(game.id, toApp ? 'app' : 'game');
    } catch {
      /* refresh below reconciles on failure */
    }
    refresh();
  }, [t, flash, refresh, setGames]);

  // Stable identity: this is passed to memoized `GameCard`s.
  const handleToggleFavorite = useCallback(async (game: Game) => {
    const next = !game.favorite;
    setGames((prev) =>
      prev.map((g) => (g.id === game.id ? { ...g, favorite: next } : g)),
    );
    try {
      await setFavorite(game.id, next);
    } catch {
      refresh();
    }
  }, [refresh, setGames]);

  // A game card was dragged onto Favoritos or a category in the sidebar.
  async function handleDropGame(target: Filter, gameId: string) {
    const game = games.find((g) => g.id === gameId);
    if (!game) return;

    if (target === 'favorites') {
      if (game.favorite) return;
      setGames((prev) =>
        prev.map((g) => (g.id === gameId ? { ...g, favorite: true } : g)),
      );
      flash(t('toast.toFavorites', { name: game.name }));
      try {
        await setFavorite(gameId, true);
      } catch {
        refresh();
      }
      return;
    }

    if (target.startsWith('cat:')) {
      const name = target.slice(4);
      const current = game.categories ?? [];
      if (current.some((c) => c.toLowerCase() === name.toLowerCase())) return;
      const next = [...current, name];
      setGames((prev) =>
        prev.map((g) => (g.id === gameId ? { ...g, categories: next } : g)),
      );
      flash(t('toast.toCategory', { name: game.name, category: name }));
      try {
        await setCategories(gameId, next);
      } catch {
        refresh();
      }
    }
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

      {introDone && splashMounted && (
        <Splash
          progress={coverProgress}
          exiting={splashExiting}
          onSkip={() => setSplashDone(true)}
        />
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
            setSelectMode(false);
            setSelectedIds(new Set());
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
          counts={counts}
          categories={categories}
          onAddCategory={() => setShowNewCategory(true)}
          onDropGame={handleDropGame}
          isDragging={dragging}
          onOpenSettings={() => setShowSettings(true)}
          onOpenHidden={() => setShowHiddenGames(true)}
          onCategoryContextMenu={(cat, x, y) => setMenu({ x, y, items: categoryMenuItems(cat) })}
          onReorderCategories={handleReorderCategories}
        />

        <main className="flex min-w-0 flex-1 flex-col">
          {selected ? (
            <DetailView
              game={selected}
              onBack={() => setSelectedId(null)}
              onLaunch={handleLaunch}
              onToggleFavorite={handleToggleFavorite}
              onToggleType={handleToggleType}
              onEditCover={setEditingCover}
              onEditCategories={setEditingCategories}
              onRemove={selected.source === 'manual' ? handleRemove : undefined}
              onHide={selected.source !== 'manual' ? handleHide : undefined}
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
                showingHome={showingHome}
                sort={sort}
                setSort={setSort}
                handleRescan={handleRescan}
                loading={loading}
                setShowNotifications={setShowNotifications}
                setShowAdd={setShowAdd}
                onStartTour={() => setShowTour(true)}
              />

              {/* Content */}
              <section className="min-h-0 flex-1 overflow-y-auto px-6 py-6">
                {filter === 'home' && !query.trim() ? (
                  <div data-tour="home">
                    <Home
                      games={games}
                      playtimes={playtimes}
                      onOpen={(g) => setSelectedId(g.id)}
                      onLaunch={handleLaunch}
                    />
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
                    selectionMode={selectMode}
                    selectedIds={selectedIds}
                    onLaunch={handleLaunch}
                    onRemove={handleRemove}
                    onHide={handleHide}
                    onEditCover={setEditingCover}
                    onToggleFavorite={handleToggleFavorite}
                    onEditCategories={setEditingCategories}
                    onDragStateChange={setDragging}
                    onOpen={handleOpen}
                    onContextMenu={handleCardContextMenu}
                    onToggleSelect={toggleSelect}
                  />
                )}
              </section>
            </>
          )}
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

      {/* Bulk action bar (multi-select) */}
      {selectMode && selectedIds.size > 0 && (
        <div className="fixed bottom-14 left-1/2 z-50 flex -translate-x-1/2 items-center gap-2 rounded-xl2 border border-line bg-elevated px-3 py-2 shadow-card">
          <span className="px-2 text-sm font-medium text-ink">
            {t('bulk.selected', { count: selectedIds.size })}
          </span>
          <button
            onClick={() => setSelectedIds(new Set(visible.map((g) => g.id)))}
            className="rounded-lg px-3 py-1.5 text-sm text-muted transition hover:bg-surface hover:text-ink"
          >
            {t('bulk.selectAll')}
          </button>
          <div className="mx-1 h-6 w-px bg-line" />
          <button
            onClick={() => bulkFavorite(true)}
            className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm text-muted transition hover:bg-surface hover:text-ink"
          >
            <StarIcon className="h-4 w-4" /> {t('bulk.favorite')}
          </button>
          <button
            onClick={() => setShowBulkCats(true)}
            className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm text-muted transition hover:bg-surface hover:text-ink"
          >
            <TagIcon className="h-4 w-4" /> {t('bulk.categories')}
          </button>
          <button
            onClick={bulkHide}
            className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm text-muted transition hover:bg-surface hover:text-destructive"
          >
            <EyeOffIcon className="h-4 w-4" /> {t('bulk.hide')}
          </button>
          <div className="mx-1 h-6 w-px bg-line" />
          <button
            onClick={exitSelection}
            className="rounded-lg px-3 py-1.5 text-sm text-muted transition hover:bg-surface hover:text-ink"
          >
            {t('bulk.cancel')}
          </button>
        </div>
      )}

      {showBulkCats && (
        <BulkCategoryDialog
          count={selectedIds.size}
          allCategories={categoryNames}
          onClose={() => setShowBulkCats(false)}
          onApply={async (cats) => {
            await bulkAddCategories(cats);
            exitSelection();
          }}
        />
      )}

      {spotlight && (
        <Spotlight
          games={games}
          onLaunch={handleLaunch}
          onClose={() => setSpotlight(false)}
        />
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

      {toast && (
        <div className="fixed bottom-14 left-1/2 -translate-x-1/2 rounded-lg border border-line bg-elevated px-4 py-2.5 text-sm text-ink shadow-card">
          {toast}
        </div>
      )}
    </div>
  );
}

function SkeletonGrid() {
  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-6">
      {Array.from({ length: 12 }).map((_, i) => (
        <div
          key={i}
          className="aspect-[2/3] animate-pulse rounded-2xl border border-line bg-elevated"
        />
      ))}
    </div>
  );
}

function Empty({
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
            className="rounded-lg bg-accent px-4 py-2.5 text-sm font-semibold text-white hover:bg-accent-soft"
          >
            {action.label}
          </button>
        )}
      </div>
    </div>
  );
}
