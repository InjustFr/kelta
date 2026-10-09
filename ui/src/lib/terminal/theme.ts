// Terminal colour themes. The same palette is applied to xterm and pushed to Rust with
// `terminal_set_palette` so the model answers OSC 4/10/11/12 queries with the colours on screen.

import type { TerminalPalette } from '$lib/gen';

export type ThemeMode = 'dark' | 'light';

interface Colors {
  foreground: string;
  background: string;
  cursor: string;
  selection: string;
  /** black, red, green, yellow, blue, magenta, cyan, white, then the 8 bright variants. */
  ansi: readonly string[];
}

const DARK: Colors = {
  foreground: '#e6e7ea',
  background: '#17181b',
  cursor: '#e6e7ea',
  selection: '#2f4a7a',
  ansi: [
    '#1d1f23',
    '#ef5b62',
    '#52c46b',
    '#f0b23e',
    '#5b93f5',
    '#c678dd',
    '#56b6c2',
    '#c9ccd3',
    '#5c6370',
    '#ff7b82',
    '#7ee08f',
    '#ffd070',
    '#82aaff',
    '#dca3ee',
    '#7fd6e0',
    '#ffffff',
  ],
};

const LIGHT: Colors = {
  foreground: '#1d1e22',
  background: '#ffffff',
  cursor: '#1d1e22',
  selection: '#bcd0f5',
  ansi: [
    '#1d1e22',
    '#c9353d',
    '#2b8a3e',
    '#8a6100',
    '#2f6fdf',
    '#9b3fb5',
    '#1b7f8c',
    '#c3c5cb',
    '#5b5e66',
    '#e0525a',
    '#3aa655',
    '#b07d00',
    '#4a86ee',
    '#b25ccb',
    '#2a9bab',
    '#8a8d96',
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

function colors(mode: ThemeMode): Colors {
  return mode === 'dark' ? DARK : LIGHT;
}

/** Palette pushed to Rust (`#rrggbb`, 16 ANSI colours). */
export function terminalPalette(mode: ThemeMode): TerminalPalette {
  const c = colors(mode);
  return { foreground: c.foreground, background: c.background, cursor: c.cursor, ansi: [...c.ansi] };
}

/** xterm `ITheme` for the mode. */
export function xtermTheme(mode: ThemeMode): Record<string, string> {
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
