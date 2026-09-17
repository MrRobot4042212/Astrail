// SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
// SPDX-License-Identifier: GPL-3.0-only
// Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

/** Spanish labels for IGDB genre and game-mode names (a finite set). Unknown
 *  values fall back to the original so nothing ever shows blank. */

const GENRES: Record<string, string> = {
  'Point-and-click': 'Point-and-click',
  Fighting: 'Lucha',
  Shooter: 'Disparos',
  Music: 'Música',
  Platform: 'Plataformas',
  Puzzle: 'Puzles',
  Racing: 'Carreras',
  'Real Time Strategy (RTS)': 'Estrategia en tiempo real',
  'Role-playing (RPG)': 'Rol (RPG)',
  Simulator: 'Simulación',
  Sport: 'Deportes',
  Strategy: 'Estrategia',
  'Turn-based strategy (TBS)': 'Estrategia por turnos',
  Tactical: 'Táctico',
  "Hack and slash/Beat 'em up": "Hack and slash / Beat 'em up",
  'Quiz/Trivia': 'Preguntas y respuestas',
  Pinball: 'Pinball',
  Adventure: 'Aventura',
  Indie: 'Indie',
  Arcade: 'Arcade',
  'Visual Novel': 'Novela visual',
  'Card & Board Game': 'Cartas y mesa',
  MOBA: 'MOBA',
};

const MODES: Record<string, string> = {
  'Single player': 'Un jugador',
  Multiplayer: 'Multijugador',
  'Co-operative': 'Cooperativo',
  'Split screen': 'Pantalla dividida',
  'Massively Multiplayer Online (MMO)': 'MMO',
  'Battle Royale': 'Battle Royale',
};

const THEMES: Record<string, string> = {
  Action: 'Acción',
  Fantasy: 'Fantasía',
  'Science fiction': 'Ciencia ficción',
  Horror: 'Terror',
  Thriller: 'Thriller',
  Survival: 'Supervivencia',
  Historical: 'Histórico',
  Stealth: 'Sigilo',
  Comedy: 'Comedia',
  Business: 'Negocios',
  Drama: 'Drama',
  'Non-fiction': 'No ficción',
  Sandbox: 'Mundo abierto',
  'Kids': 'Infantil',
  Educational: 'Educativo',
  Mystery: 'Misterio',
  Romance: 'Romance',
  Warfare: 'Bélico',
  Party: 'Fiesta',
  '4X (explore, expand, exploit, and exterminate)': '4X',
  'Open world': 'Mundo abierto',
};

const PERSPECTIVES: Record<string, string> = {
  'First person': 'Primera persona',
  'Third person': 'Tercera persona',
  'Bird view / Isometric': 'Cenital / Isométrica',
  'Side view': 'Lateral',
  Text: 'Texto',
  Auditory: 'Auditiva',
  'Virtual Reality': 'Realidad virtual',
};

import i18n from '@/i18n/config';

// IGDB returns these names in English. We only localize them when the UI is in
// Spanish; in English we keep the original value.
const isEs = () => i18n.language?.startsWith('es');

export const translateGenre = (g: string): string => (isEs() ? GENRES[g] ?? g : g);
export const translateMode = (m: string): string => (isEs() ? MODES[m] ?? m : m);
export const translateTheme = (t: string): string => (isEs() ? THEMES[t] ?? t : t);
export const translatePerspective = (p: string): string =>
  isEs() ? PERSPECTIVES[p] ?? p : p;
