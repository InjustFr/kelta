// Terminal colour themes. The same palette is applied to xterm and pushed to Rust with
// `terminal_set_palette` so the model answers OSC 4/10/11/12 queries with the colours on screen.
// Source of truth for the Bezel terminal palettes (DESIGN §5); tokens.css mirrors bg/fg/cursor/selection.
// The cursor is neutral: switching project never re-pushes the palette. A selected theme passes
// its resolved `Colors` instead of a mode (lib/theme).

import type { TerminalPalette } from '$lib/gen';

export type ThemeMode = 'dark' | 'light';

export interface Colors {
  foreground: string;
  background: string;
  cursor: string;
  selection: string;
  /** black, red, green, yellow, blue, magenta, cyan, white, then the 8 bright variants. */
  ansi: readonly string[];
}

const DARK: Colors = {
  foreground: '#d3d9df',
  background: '#121519',
  cursor: '#d3d9df',
  selection: '#2c3f52',
  ansi: [
    '#1e232a',
    '#e0675f',
    '#79b88a',
    '#d6b163',
    '#6e9bd1',
    '#b48bcb',
    '#5fb3b8',
    '#c4cbd3',
    '#5e6873',
    '#f08a82',
    '#96d2a6',
    '#e9ca86',
    '#92b6e6',
    '#cba8de',
    '#84cbcf',
    '#eef1f4',
  ],
};

const LIGHT: Colors = {
  foreground: '#1b2026',
  background: '#fafbfb',
  cursor: '#1b2026',
  selection: '#c9daea',
  ansi: [
    '#1b2026',
    '#b8352e',
    '#2f7a47',
    '#8a6400',
    '#2e62a8',
    '#87459e',
    '#1d7a80',
    '#b9c0c7',
    '#5f6873',
    '#d24a42',
    '#3e9259',
    '#a87b00',
    '#3f78c2',
    '#a05cb8',
    '#2a9299',
    '#858e98',
  ],
};

const NAMES = [
  'black',
  'red',
  'green',
  'yellow',
  'blue',
  'magenta',
  'cyan',
  'white',
  'brightBlack',
  'brightRed',
  'brightGreen',
  'brightYellow',
  'brightBlue',
  'brightMagenta',
  'brightCyan',
  'brightWhite',
] as const;

/** The Bezel base palette for a mode, or the given (theme-resolved) colours. */
export function colors(theme: ThemeMode | Colors): Colors {
  if (typeof theme !== 'string') return theme;
  return theme === 'dark' ? DARK : LIGHT;
}

/** Palette pushed to Rust (`#rrggbb`, 16 ANSI colours). */
export function terminalPalette(mode: ThemeMode | Colors): TerminalPalette {
  const c = colors(mode);
  return { foreground: c.foreground, background: c.background, cursor: c.cursor, ansi: [...c.ansi] };
}

/** xterm `ITheme` for the mode or the resolved colours. */
export function xtermTheme(mode: ThemeMode | Colors): Record<string, string> {
  const c = colors(mode);
  const theme: Record<string, string> = {
    foreground: c.foreground,
    background: c.background,
    cursor: c.cursor,
    cursorAccent: c.background,
    selectionBackground: c.selection,
    selectionInactiveBackground: c.selection,
  };
  NAMES.forEach((name, i) => {
    theme[name] = c.ansi[i]!;
  });
  return theme;
}
