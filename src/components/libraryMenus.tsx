// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

// Right-click menus of the library screen, built from the entry and a set of
// actions so the screen itself carries no menu markup.

import type { TFunction } from 'i18next';

import { folderOf } from '@/lib/libraryView';
import type { Category, Game } from '@/lib/types';
import type { MenuItem } from './ContextMenu';
import {
  AppIcon,
  EyeOffIcon,
  FolderIcon,
  GridIcon,
  ImageIcon,
  PencilIcon,
  PlayIcon,
  StarIcon,
  TagIcon,
  TrashIcon,
} from './icons';

export interface GameMenuActions {
  launch: (game: Game) => void;
  toggleFavorite: (game: Game) => void;
  toggleType: (game: Game) => void;
  editCategories: (game: Game) => void;
  editCover: (game: Game) => void;
  openFolder: (game: Game) => void;
  remove: (game: Game) => void;
  hide: (game: Game) => void;
}

/** Menu of a card. Manual entries can be removed; scanned ones only hidden,
 *  since the next scan would bring them back. */
export function gameMenuItems(game: Game, t: TFunction, on: GameMenuActions): MenuItem[] {
  const items: MenuItem[] = [
    { label: t('menu.play'), icon: <PlayIcon className="h-4 w-4" />, onClick: () => on.launch(game) },
    {
      label: game.favorite ? t('menu.removeFavorite') : t('menu.addFavorite'),
      icon: <StarIcon className="h-4 w-4" fill={game.favorite ? 'currentColor' : 'none'} />,
      onClick: () => on.toggleFavorite(game),
    },
    { label: t('menu.categories'), icon: <TagIcon className="h-4 w-4" />, onClick: () => on.editCategories(game) },
    { label: t('menu.changeCover'), icon: <ImageIcon className="h-4 w-4" />, onClick: () => on.editCover(game) },
    game.source === 'app'
      ? { label: t('menu.markAsGame'), icon: <GridIcon className="h-4 w-4" />, onClick: () => on.toggleType(game) }
      : { label: t('menu.markAsApp'), icon: <AppIcon className="h-4 w-4" />, onClick: () => on.toggleType(game) },
  ];
  if (folderOf(game)) {
    items.push({
      label: t('menu.openFolder'),
      icon: <FolderIcon className="h-4 w-4" />,
      onClick: () => on.openFolder(game),
    });
  }
  items.push({ type: 'separator' });
  if (game.source === 'manual') {
    items.push({ label: t('menu.remove'), danger: true, icon: <TrashIcon className="h-4 w-4" />, onClick: () => on.remove(game) });
  } else {
    items.push({ label: t('menu.hide'), danger: true, icon: <EyeOffIcon className="h-4 w-4" />, onClick: () => on.hide(game) });
  }
  return items;
}

/** Menu of a custom category in the sidebar. */
export function categoryMenuItems(
  cat: Category,
  t: TFunction,
  on: { edit: (cat: Category) => void; remove: (cat: Category) => void },
): MenuItem[] {
  return [
    { label: t('menu.edit'), icon: <PencilIcon className="h-4 w-4" />, onClick: () => on.edit(cat) },
    { type: 'separator' },
    {
      label: t('menu.deleteCategory'),
      danger: true,
      icon: <TrashIcon className="h-4 w-4" />,
      onClick: () => on.remove(cat),
    },
  ];
}
