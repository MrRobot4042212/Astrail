// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

'use client';

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { GameSource, Category } from '@/lib/types';
import { SOURCE_META, SOURCE_ORDER } from '@/lib/sources';
import { CATEGORY_ICONS } from '@/lib/categoryIcons';
import {
  GearIcon,
  EyeOffIcon,
  GridIcon, StarIcon, TagIcon, PlusIcon, AppIcon, HomeIcon
} from './icons';

export type Filter = 'home' | 'all' | 'favorites' | GameSource | `cat:${string}`;

type NavItem = { id: Filter; label: string; Icon: typeof GridIcon };

export function Sidebar({
  filter,
  onFilter,
  counts,
  categories,
  onAddCategory,
  onDropGame,
  isDragging = false,
  onOpenSettings,
  onOpenHidden,
  onCategoryContextMenu,
  onReorderCategories,
}: {
  filter: Filter;
  onFilter: (f: Filter) => void;
  counts: Record<string, number>;
  /** User categories (with icons), in the user-defined order. */
  categories: Category[];
  onAddCategory: () => void;
  /** A game card was dropped onto a droppable target (Favoritos / a category). */
  onDropGame: (target: Filter, gameId: string) => void;
  /** True while a game card is being dragged, to invite valid drop targets. */
  isDragging?: boolean;
  onOpenSettings: () => void;
  onOpenHidden: () => void;
  /** Right-click a category → context menu (edit / delete) at the cursor. */
  onCategoryContextMenu: (c: Category, x: number, y: number) => void;
  /** Persist a new category order (full ordered list of names). */
  onReorderCategories: (names: string[]) => void;
}) {
  const { t } = useTranslation();
  // Which droppable target the dragged card is currently hovering, for highlight.
  const [dragOver, setDragOver] = useState<Filter | null>(null);
  // Name of the category being dragged to reorder (vs. a game being assigned).
  const [dragCat, setDragCat] = useState<string | null>(null);

  /** Move the dragged category to the dropped-on category's position. */
  function reorderTo(targetName: string) {
    if (!dragCat || dragCat === targetName) return;
    const names = categories.map((c) => c.name);
    const from = names.indexOf(dragCat);
    const to = names.indexOf(targetName);
    if (from < 0 || to < 0) return;
    names.splice(to, 0, names.splice(from, 1)[0]);
    onReorderCategories(names);
  }

  // Drop handlers for a target id; only attached to Favoritos and categories.
  const dropProps = (id: Filter) => ({
    onDragOver: (e: React.DragEvent) => {
      e.preventDefault();
      e.dataTransfer.dropEffect = 'copy';
      if (dragOver !== id) setDragOver(id);
    },
    onDragLeave: () => setDragOver((d) => (d === id ? null : d)),
    onDrop: (e: React.DragEvent) => {
      e.preventDefault();
      const gameId = e.dataTransfer.getData('text/plain');
      setDragOver(null);
      if (gameId) onDropGame(id, gameId);
    },
  });

  // Group 1 — the library itself: everything + favorites. Favoritos is always
  // shown so it can act as a permanent drop target (drag a game in to favorite).
  const libraryItems: NavItem[] = [
    { id: 'home', label: t('sidebar.home'), Icon: HomeIcon },
    { id: 'all', label: t('sidebar.all'), Icon: GridIcon },
    { id: 'favorites', label: t('sidebar.favorites'), Icon: StarIcon },
  ];

  // Group 2 — the app's built-in store filters (only non-empty ones). Apps are
  // split into their own group below, so exclude them here.
  const storeItems: NavItem[] = SOURCE_ORDER.filter((s) => s !== 'app' && counts[s] > 0).map(
    (s) => ({
      id: s as Filter,
      label: SOURCE_META[s].label,
      Icon: SOURCE_META[s].Icon,
    }),
  );

  // Base row styling, plus drop-target states: a quiet dashed "ready" hint while
  // any drag is in progress, and a strong accent highlight when hovered over.
  const itemClass = (id: Filter, droppable: boolean) => {
    const active = filter === id;
    const over = dragOver === id;
    const ready = droppable && isDragging && !over;
    return [
      'group flex items-center gap-3 px-3 py-2.5 text-sm transition-all duration-150',
      over
        ? 'bg-accent/15 text-ink ring-2 ring-inset ring-accent scale-[1.02]'
        : ready
          ? 'text-ink bg-accent/5 outline-dashed outline-1 outline-accent/40 -outline-offset-2'
          : active
            ? 'bg-elevated text-ink'
            : 'text-muted hover:bg-elevated/50 hover:text-ink',
    ].join(' ');
  };

  const renderItem = ({ id, label, Icon }: NavItem, droppable = false) => {
    const active = filter === id;
    const over = dragOver === id;
    return (
      <button
        key={id}
        onClick={() => onFilter(id)}
        {...(droppable ? dropProps(id) : {})}
        className={itemClass(id, droppable)}
      >
        <Icon
          className={`h-[18px] w-[18px] transition-transform ${active || over ? 'text-accent' : ''
            } ${over ? 'scale-125' : ''}`}
        />
        <span className="flex-1 truncate text-left">{label}</span>
        {id !== 'home' && (
          <span className="text-xs tabular-nums text-muted">{counts[id] ?? 0}</span>
        )}
      </button>
    );
  };

  const Heading = ({ children }: { children: React.ReactNode }) => (
    <p className="px-3 pb-1 pt-5 text-[11px] font-semibold uppercase tracking-wide text-primary">
      {children}
    </p>
  );

  return (
    <aside data-tour="sidebar" className="flex w-[208px] shrink-0 flex-col overflow-y-auto border-r border-line bg-sidebar px-3 py-5">
      <div className="mb-3 flex items-center gap-2.5 px-2">
        <svg xmlns="http://www.w3.org/2000/svg" width="24px" height="24px" viewBox="0 0 256 251">
          <path d="M0 0h256v251H0z" fill="none" />
          <path fill="#df4f4f" d="M.439.438L219.3 232.266s7.457 5.259 13.158-.877c5.702-6.135 1.316-12.27 1.316-12.27zM69.738 22.35l166.668 179.677s7.456 5.26 13.158-.876c5.702-6.135 1.316-12.27 1.316-12.27zM21.053 69.242L187.72 248.919s7.456 5.259 13.158-.877c5.702-6.135 1.316-12.27 1.316-12.27zM128.32 41.194l116.442 125.53s5.21 3.674 9.193-.612c3.983-4.287.9１９-8．５７３．９１９-8．５７３zm-9１．２２８ 8２．３８９l１１６．４４１ １２５．５３s５．２１ ３．６７４ ９．１９３-.６１３c３．９８３-４．２８６．９１９-８．５７２．９１９-８．５７２zM１８８．１６ ６８．３６５l５２．７７５ ５７．０６７s２．５７７ １．７２２ ４．５４７-.２８７s．４５５-４．０１７．４５５-４．０１７zM６６．２２９ １８１．４３l５２．７７５ ５７．０６７s２．５７７ １．７２２ ４．５４７-.２８６s．４５５-４．０１７．４５５-４．０１７z" />
        </svg>
        <span className="font-display text-lg font-semibold tracking-tight text-ink">
          Meteor
        </span>
      </div>

      {/* Group: Biblioteca (Favoritos is a drop target) */}
      <Heading>{t('sidebar.library')}</Heading>
      <nav className="flex flex-col gap-1">
        {libraryItems.map((it) => renderItem(it, it.id === 'favorites'))}
      </nav>

      {/* Group: Proveedores (built-in / default) */}
      {storeItems.length > 0 && (
        <>
          <Heading>{t('sidebar.platforms')}</Heading>
          <nav className="flex flex-col gap-1">{storeItems.map((it) => renderItem(it))}</nav>
        </>
      )}

      {/* Group: Aplicaciones (auto-detected non-game apps) */}
      {counts.app > 0 && (
        <>
          <Heading>{t('sidebar.applications')}</Heading>
          <nav className="flex flex-col gap-1">
            {renderItem({ id: 'app', label: t('sidebar.apps'), Icon: AppIcon })}
          </nav>
        </>
      )}

      {/* Group: Categorías (user-created, drop targets), separated from defaults */}
      <div data-tour="categories" className="flex items-center justify-between pr-1">
        <Heading>{t('sidebar.customCategories')}</Heading>
        <button
          onClick={onAddCategory}
          title={t('sidebar.newCategory')}
          className="mt-3 grid h-5 w-5 place-items-center text-muted transition hover:bg-elevated hover:text-accent"
        >
          <PlusIcon className="h-3.5 w-3.5" />
        </button>
      </div>
      {categories.length > 0 ? (
        <nav className="flex flex-col gap-1">
          {categories.map((cat) => {
            const { name, icon } = cat;
            const id = `cat:${name}` as Filter;
            const over = dragOver === id;
            const Icon = (icon && CATEGORY_ICONS[icon]) || TagIcon;
            return (
              <div
                key={id}
                // Draggable to reorder; also a drop target for both a reordered
                // category and a game card being assigned.
                draggable
                onDragStart={(e) => {
                  setDragCat(name);
                  e.dataTransfer.setData('application/x-meteor-cat', name);
                  e.dataTransfer.effectAllowed = 'move';
                }}
                onDragEnd={() => setDragCat(null)}
                onDragOver={(e) => {
                  e.preventDefault();
                  const reordering = e.dataTransfer.types.includes('application/x-meteor-cat');
                  e.dataTransfer.dropEffect = reordering ? 'move' : 'copy';
                  if (dragOver !== id) setDragOver(id);
                }}
                onDragLeave={() => setDragOver((d) => (d === id ? null : d))}
                onDrop={(e) => {
                  e.preventDefault();
                  setDragOver(null);
                  if (e.dataTransfer.types.includes('application/x-meteor-cat')) {
                    reorderTo(name);
                  } else {
                    const gameId = e.dataTransfer.getData('text/plain');
                    if (gameId) onDropGame(id, gameId);
                  }
                }}
                onContextMenu={(e) => {
                  e.preventDefault();
                  onCategoryContextMenu(cat, e.clientX, e.clientY);
                }}
                className={`${itemClass(id, true)} ${dragCat === name ? 'opacity-40' : ''}`}
              >
                <button
                  onClick={() => onFilter(id)}
                  className="flex min-w-0 flex-1 items-center gap-3 text-left"
                >
                  <Icon
                    className={`h-[18px] w-[18px] shrink-0 transition-transform ${filter === id || over ? 'text-accent' : ''
                      } ${over ? 'scale-125' : ''}`}
                  />
                  <span className="flex-1 truncate">{name}</span>
                </button>
                <span className="text-xs tabular-nums text-muted">{counts[id] ?? 0}</span>
              </div>
            );
          })}
        </nav>
      ) : (
        <p className="px-3 py-1 text-xs leading-relaxed text-muted/60">
          {t('sidebar.categoriesEmpty')}
        </p>
      )}

      <div className="mt-auto flex flex-col gap-1 pt-5">
        <button
          onClick={onAddCategory}
          className="flex items-center gap-3 px-3 py-2.5 text-sm text-muted transition-colors hover:bg-elevated/50 hover:text-ink"
        >
          <PlusIcon className="h-[18px] w-[18px]" />
          <span>{t('sidebar.newCategory')}</span>
        </button>
        <hr />
        <button
          onClick={onOpenHidden}
          className="flex items-center gap-3 px-3 py-2.5 text-sm text-muted transition-colors hover:bg-elevated/50 hover:text-ink"
        >
          <EyeOffIcon className="h-[18px] w-[18px]" />
          <span>{t('sidebar.hiddenItems')}</span>
        </button>
        <button
          data-tour="settings-btn"
          onClick={onOpenSettings}
          className="flex items-center gap-3 px-3 py-2.5 text-sm text-muted transition-colors hover:bg-elevated/50 hover:text-ink"
        >
          <GearIcon className="h-[18px] w-[18px]" />
          <span>{t('sidebar.settings')}</span>
        </button>
        <span className="text-xs text-muted border-t border-line pt-2 mt-2 text-center">
          {t('sidebar.madeBy')} <a href="https://github.com/MrRobot4042212" className="text-underline hover:text-accent">
            Dalfon_dev
          </a>
        </span>
      </div>
    </aside>
  );
}
