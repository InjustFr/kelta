// Built-in themes. Bezel overrides nothing: its values stay in tokens.css and terminal/theme.ts.

import type { ThemeDef, ThemeTerminal, ThemeUi } from '$lib/gen';

// Every token unset (the generated types spell unset as null).
const NONE = { ui: {} as ThemeUi, terminal: {} as ThemeTerminal };

export const BUILTIN: Record<string, ThemeDef> = {
  'bezel-dark': { name: 'Bezel Dark', base: 'dark', ...NONE },
  'bezel-light': { name: 'Bezel Light', base: 'light', ...NONE },
};
