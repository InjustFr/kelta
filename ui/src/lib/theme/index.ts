// Resolves the theme of the current mode (`app.dark_theme` / `app.light_theme`) into inline
// `--k-*` overrides on top of tokens.css and the terminal colours (DESIGN §5, §9).

import type { Settings, ThemeDef, ThemeUi } from '$lib/gen';
import { colors, type Colors, type ThemeMode } from '$lib/terminal/theme';

import { BUILTIN } from './builtin';

export interface ResolvedTheme {
  id: string;
  def: ThemeDef;
  /** Inline custom properties for <html>; empty for Bezel. */
  vars: Record<string, string>;
  /** Hue for --k-project when the active project has no colour (still goes through projectAccent). */
  accent: string | null;
  terminal: Colors;
}

/** `#rrggbb` lowercased, or null for anything else (defence behind the schema pattern). */
function hex(v: string | null | undefined): string | null {
  return typeof v === 'string' && /^#[0-9a-f]{6}$/i.test(v) ? v.toLowerCase() : null;
}

export function resolveTheme(settings: Settings | null | undefined, mode: ThemeMode): ResolvedTheme {
  const fallback = `bezel-${mode}`;
  let id = (mode === 'dark' ? settings?.app.dark_theme : settings?.app.light_theme) ?? fallback;
  let def = settings?.themes?.[id] ?? BUILTIN[id];
  // An unknown id or a theme for the other mode would leave the UI half-themed.
  if (!def || def.base !== mode) {
    id = fallback;
    def = BUILTIN[fallback]!;
  }

  const vars: Record<string, string> = {};
  for (const [key, value] of Object.entries(def.ui ?? {}) as [keyof ThemeUi, string | null][]) {
    const v = hex(value);
    if (v && key !== 'accent') vars[`--k-${key.replace(/_/g, '-')}`] = v;
  }

  const base = colors(mode);
  const t = def.terminal ?? {};
  const set = {
    background: hex(t.background) ?? hex(def.ui?.well),
    foreground: hex(t.foreground) ?? hex(def.ui?.fg),
    cursor: hex(t.cursor),
    selection: hex(t.selection),
  };
  const terminal: Colors = {
    background: set.background ?? base.background,
    foreground: set.foreground ?? base.foreground,
    cursor: set.cursor ?? base.cursor,
    selection: set.selection ?? base.selection,
    ansi: t.ansi?.length === 16 ? t.ansi.map((c, i) => hex(c) ?? base.ansi[i]!) : base.ansi,
  };
  if (set.background) vars['--k-term-bg'] = set.background;
  if (set.foreground) vars['--k-term-fg'] = set.foreground;
  if (set.cursor) vars['--k-term-cursor'] = set.cursor;
  if (set.selection) vars['--k-term-selection'] = set.selection;

  return { id, def, vars, accent: hex(def.ui?.accent), terminal };
}
