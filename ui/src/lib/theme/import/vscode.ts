// VS Code colour theme (JSON / JSONC) -> Kelta theme (#231). Only `colors` is read: Kelta has no
// editor, so `tokenColors` / `semanticTokenColors` are ignored. Loaded on demand by the picker.

import type { ThemeBase, ThemeDef, ThemeTerminal, ThemeUi } from '$lib/gen';

import { BUILTIN } from '../builtin';
import { complete, parse, type PartialTheme } from './derive';

// First present key wins; complete() derives the rest.
const UI: Record<keyof ThemeUi, string[]> = {
  well: ['editor.background'],
  bezel: ['sideBar.background', 'activityBar.background'],
  bezel_raised: ['tab.inactiveBackground', 'editorGroupHeader.tabsBackground', 'titleBar.activeBackground'],
  bg_float: ['editorWidget.background', 'quickInput.background', 'menu.background'],
  fg: ['editor.foreground', 'foreground'],
  fg_muted: ['descriptionForeground'],
  fg_subtle: ['tab.inactiveForeground'],
  fg_chrome: ['sideBar.foreground', 'sideBarTitle.foreground'],
  border: ['panel.border', 'editorGroup.border', 'sideBar.border', 'widget.border'],
  border_strong: ['input.border', 'contrastBorder'],
  accent: ['focusBorder', 'button.background', 'activityBarBadge.background', 'textLink.foreground'],
  danger: ['errorForeground', 'editorError.foreground'],
  warn: ['editorWarning.foreground', 'list.warningForeground'],
  ok: ['gitDecoration.addedResourceForeground'],
  info: ['editorInfo.foreground'],
};
const TERMINAL: Record<Exclude<keyof ThemeTerminal, 'ansi'>, string[]> = {
  background: ['terminal.background'],
  foreground: ['terminal.foreground'],
  cursor: ['terminalCursor.foreground', 'editorCursor.foreground'],
  selection: ['terminal.selectionBackground', 'editor.selectionBackground'],
};
const HUES = ['Black', 'Red', 'Green', 'Yellow', 'Blue', 'Magenta', 'Cyan', 'White'];
const ANSI = [...HUES.map((h) => `terminal.ansi${h}`), ...HUES.map((h) => `terminal.ansiBright${h}`)];
const BASES: Record<string, ThemeBase> = {
  dark: 'dark',
  hc: 'dark',
  hcDark: 'dark',
  light: 'light',
  hcLight: 'light',
};

/** The mapping core, shared with other sources of VS Code `colors` (#232). */
export function fromVscodeColors(
  colors: Record<string, unknown>,
  name: string,
  type?: unknown,
): ThemeDef | Error {
  let found = 0;
  const pick = <K extends string>(map: Record<K, string[]>) => {
    const out: Partial<Record<K, string>> = {};
    for (const [token, keys] of Object.entries(map) as [K, string[]][]) {
      const key = keys.find((k) => parse(colors[k]));
      if (key) out[token] = colors[key] as string;
    }
    found += Object.keys(out).length;
    return out;
  };
  const ui = pick(UI);
  const terminal: PartialTheme['terminal'] = pick(TERMINAL);
  terminal.ansi = ANSI.map((k) => (parse(colors[k]) ? (colors[k] as string) : undefined));
  found += terminal.ansi.filter(Boolean).length;
  if (!found) return new Error('No usable colours in this theme');
  const base = typeof type === 'string' && Object.hasOwn(BASES, type) ? BASES[type] : undefined;
  return complete({ name, ui, terminal, ...(base && { base }) });
}

/** Parses a theme file. Errors carry the message shown to the user. */
export function fromVscode(text: string, fileName: string): ThemeDef | Error {
  let json: unknown;
  try {
    json = JSON.parse(stripJsonc(text));
  } catch {
    return new Error('This file is not valid JSON');
  }
  const theme = (json && typeof json === 'object' && !Array.isArray(json) ? json : {}) as Record<
    string,
    unknown
  >;
  if (theme.include !== undefined) {
    return new Error(
      `This theme extends ${String(theme.include)}. In VS Code run 'Developer: Generate Color Theme From Current Settings' and import that file.`,
    );
  }
  const colors = theme.colors;
  if (!colors || typeof colors !== 'object' || Array.isArray(colors)) {
    return new Error('No "colors" object in this theme');
  }
  const name =
    typeof theme.name === 'string' && theme.name.trim() ? theme.name.trim() : titleCase(stem(fileName));
  return fromVscodeColors(colors as Record<string, unknown>, name, theme.type);
}

/** Kebab id for `[themes.<id>]` (no dots: settings paths split on them), never a built-in id. */
export function themeId(name: string, fileName: string): string {
  const id = slug(name) || slug(stem(fileName)) || 'imported';
  return id in BUILTIN ? `${id}-custom` : id;
}

const stem = (fileName: string) => fileName.replace(/^.*[\\/]/, '').replace(/\.jsonc?$/i, '');
const titleCase = (s: string) =>
  s
    .split(/[-_.\s]+/)
    .filter(Boolean)
    .map((w) => w[0]!.toUpperCase() + w.slice(1))
    .join(' ') || s;
const slug = (s: string) =>
  s
    .normalize('NFKD')
    .replace(/\p{M}/gu, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');

/** JSONC -> JSON: drops comments and trailing commas, leaves strings untouched. */
export function stripJsonc(text: string): string {
  let out = '';
  for (let i = 0; i < text.length; i++) {
    const c = text[i]!;
    if (c === '"') {
      const start = i;
      for (i++; i < text.length && text[i] !== '"'; i++) if (text[i] === '\\') i++;
      out += text.slice(start, i + 1);
    } else if (c === '/' && text[i + 1] === '/') {
      const end = text.indexOf('\n', i);
      i = end < 0 ? text.length : end - 1;
    } else if (c === '/' && text[i + 1] === '*') {
      const end = text.indexOf('*/', i + 2);
      i = end < 0 ? text.length : end + 1;
      out += ' ';
    } else {
      if (c === '}' || c === ']') {
        let j = out.length - 1;
        while (j >= 0 && /\s/.test(out[j]!)) j--;
        if (out[j] === ',') out = out.slice(0, j) + out.slice(j + 1);
      }
      out += c;
    }
  }
  return out;
}
