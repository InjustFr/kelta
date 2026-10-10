// Terminal colour schemes (iTerm2, Ghostty, Kitty, Alacritty, base16) -> VS Code `colors` keys ->
// fromVscodeColors() (#232). Detected by content: Ghostty theme files have no extension.
// shortcut: Alacritty dotted keys and inline tables are not read, only `[section]` headers.

import type { ThemeDef } from '$lib/gen';

import { ANSI, fromVscode, fromVscodeColors, HUES, stem, titleCase } from './vscode';

export { themeId } from './vscode';

export type Format = 'vscode' | 'iterm' | 'alacritty' | 'base16' | 'ghostty' | 'kitty';

export const UNRECOGNISED =
  'Kelta could not recognise this theme file. Supported: VS Code, iTerm2, Ghostty, Kitty, Alacritty, base16.';

/** First match wins, so the `background` lines shared by several formats are never ambiguous. */
export function detect(text: string): Format | null {
  if (/^(?:\s|\/\/.*|\/\*[\s\S]*?\*\/)*\{/.test(text)) return 'vscode'; // JSONC may open with comments
  if (text.includes('<plist') && text.includes('Ansi 0 Color')) return 'iterm';
  if (/^\s*\[colors\./m.test(text)) return 'alacritty';
  if (/^\s*base00\s*:/im.test(text)) return 'base16';
  if (/^\s*palette\s*=\s*\d+\s*=/m.test(text)) return 'ghostty';
  if (/^\s*color\d+\s+[^\s=]/m.test(text)) return 'kitty';
  return null;
}

// VS Code key -> the file's own key (lowercase lookup). Terminal-only formats also colour the editor,
// so the whole window matches the terminal.
const term = (ansi: (i: number) => string, bg: string, fg: string, cursor: string, selection: string) => ({
  'editor.background': bg,
  'editor.foreground': fg,
  'terminal.background': bg,
  'terminal.foreground': fg,
  'terminalCursor.foreground': cursor,
  'terminal.selectionBackground': selection,
  ...Object.fromEntries(ANSI.map((k, i) => [k, ansi(i)])),
});
// base16-shell order.
const B16_ANSI = '00 08 0b 0a 0d 0e 0c 05 03 08 0b 0a 0d 0e 0c 07'.split(' ');
const KEYS: Record<Exclude<Format, 'vscode'>, Record<string, string>> = {
  iterm: term(
    (i) => `Ansi ${i} Color`,
    'Background Color',
    'Foreground Color',
    'Cursor Color',
    'Selection Color',
  ),
  ghostty: term((i) => `palette.${i}`, 'background', 'foreground', 'cursor-color', 'selection-background'),
  kitty: term((i) => `color${i}`, 'background', 'foreground', 'cursor', 'selection_background'),
  alacritty: term(
    (i) => `colors.${i < 8 ? 'normal' : 'bright'}.${HUES[i % 8]}`,
    'colors.primary.background',
    'colors.primary.foreground',
    'colors.cursor.cursor',
    'colors.selection.background',
  ),
  base16: {
    ...term((i) => `base${B16_ANSI[i]}`, 'base00', 'base05', 'base05', 'base02'),
    'tab.inactiveBackground': 'base01', // bezel_raised
    'panel.border': 'base02',
    'tab.inactiveForeground': 'base04', // fg_subtle
    focusBorder: 'base0d', // accent
  },
};

/** Parses any supported theme file. Errors carry the message shown to the user. */
export function fromThemeFile(text: string, fileName: string): ThemeDef | Error {
  const format = detect(text);
  if (format === 'vscode') return fromVscode(text, fileName);
  if (!format) return new Error(UNRECOGNISED);
  const vals = format === 'iterm' ? readPlist(text) : readLines(text);
  const colors: Record<string, string> = {};
  for (const [key, own] of Object.entries(KEYS[format])) {
    const v = vals[own.toLowerCase()];
    const hex = v && /^(?:#|0x)?([0-9a-f]{3,8})$/i.exec(v);
    if (v) colors[key] = hex ? `#${hex[1]}` : v; // anything else is dropped by fromVscodeColors
  }
  const own = format === 'base16' ? (vals.name || vals.scheme)?.trim() : undefined;
  return fromVscodeColors(
    colors,
    own || titleCase(stem(fileName)),
    format === 'base16' ? vals.variant : undefined,
  );
}

/** `key = value`, `key: value` or `key value` lines; keys lowercased, `[section]` and Ghostty's `N=` prefixed. */
function readLines(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  let section = '';
  for (const line of text.split(/\r?\n/)) {
    const head = /^\s*\[([^\]]+)\]/.exec(line);
    if (head) section = `${head[1]!.trim()}.`;
    const m = !head && /^\s*([\w.-]+)(?:\s*[=:]\s*|\s+)(.*)$/.exec(line);
    if (!m) continue;
    let [key, v] = [m[1]!, /^(["'])(.*?)\1/.exec(m[2]!)?.[2] ?? m[2]!.replace(/\s+#.*$/, '').trim()];
    const slot = key === 'palette' && /^(\d+)\s*=\s*(\S+)/.exec(v);
    if (slot) [key, v] = [`palette.${slot[1]}`, slot[2]!];
    out[(section + key).toLowerCase()] = v;
  }
  return out;
}

/** iTerm2 plist: the root `<dict>` alternates `<key>` and colour `<dict>` (float components 0-1). */
function readPlist(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  const root = [
    ...(new DOMParser().parseFromString(text, 'application/xml').querySelector('plist > dict')?.children ??
      []),
  ];
  for (let i = 0; i + 1 < root.length; i += 2) {
    const fields = [...root[i + 1]!.children];
    const comp = (c: string) => {
      const j = fields.findIndex((f) => f.tagName === 'key' && f.textContent === `${c} Component`);
      return j < 0 ? NaN : Number(fields[j + 1]?.textContent);
    };
    const rgb = ['Red', 'Green', 'Blue'].map(comp);
    if (rgb.every(Number.isFinite)) {
      out[root[i]!.textContent!.toLowerCase()] = `#${rgb
        .map((x) =>
          Math.round(Math.min(1, Math.max(0, x)) * 255)
            .toString(16)
            .padStart(2, '0'),
        )
        .join('')}`;
    }
  }
  return out;
}
