// Turns the colours found in a foreign theme into a full Kelta theme: every stored value is
// `#rrggbb`, alpha is composited, and every missing token is derived (a half-Bezel chrome looks broken).

import type { ThemeBase, ThemeDef, ThemeTerminal, ThemeUi } from '$lib/gen';
import { colors } from '$lib/terminal/theme';

import { linear, parseHex } from '../hex';

/** Raw colours (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`) as the source gives them; any may be absent. */
export interface PartialTheme {
  name: string;
  base?: ThemeBase;
  ui: Partial<Record<keyof ThemeUi, string>>;
  terminal: Partial<Record<Exclude<keyof ThemeTerminal, 'ansi'>, string>> & { ansi?: (string | undefined)[] };
}

// Bezel's ui fg from tokens.css (its well is the terminal background in lib/terminal/theme.ts).
const BEZEL_FG: Record<ThemeBase, string> = { dark: '#e1e6ec', light: '#161b21' };

const toHex = (c: readonly number[]) =>
  `#${c.map((x) => Math.round(x).toString(16).padStart(2, '0')).join('')}`;
const rgb = (h: string) => parseHex(h)!.slice(0, 3);

/** Plain sRGB lerp from `a` to `b` (`#rrggbb`). */
export function mix(a: string, b: string, p: number): string {
  const x = rgb(a);
  const y = rgb(b);
  return toHex(x.map((v, i) => v + (y[i]! - v) * p));
}

/** `v` as `#rrggbb`, its alpha composited over `under`; null when unparsable. */
function over(v: string | undefined, under: string): string | null {
  const c = parseHex(v);
  return c && mix(under, toHex(c.slice(0, 3)), c[3]);
}

/** WCAG relative luminance. */
export function luminance(hex: string): number {
  const [r, g, b] = rgb(hex).map(linear);
  return 0.2126 * r! + 0.7152 * g! + 0.0722 * b!;
}

export function complete(p: PartialTheme): ThemeDef {
  const guess = over(p.ui.well, colors('dark').background);
  const base = p.base ?? (guess && luminance(guess) >= 0.2 ? 'light' : 'dark');
  const bezel = colors(base);
  const well = over(p.ui.well, bezel.background) ?? bezel.background;
  const on = (k: keyof ThemeUi, derived: string) => over(p.ui[k], well) ?? derived;

  const fg = on('fg', BEZEL_FG[base]);
  const t = p.terminal;
  const background = over(t.background, well) ?? well;
  const ansi = bezel.ansi.map((c, i) => over(t.ansi?.[i], background) ?? c);
  const raised = on('bezel_raised', mix(well, fg, 0.1));
  const muted = on('fg_muted', mix(fg, well, 0.25));
  const accent = on('accent', ansi[4]!);

  const ui: ThemeUi = {
    bezel: on('bezel', mix(well, fg, 0.06)),
    bezel_raised: raised,
    well,
    bg_float: on('bg_float', raised),
    fg,
    fg_muted: muted,
    fg_subtle: on('fg_subtle', mix(fg, well, 0.38)),
    fg_chrome: on('fg_chrome', muted),
    border: on('border', mix(well, fg, 0.15)),
    border_strong: on('border_strong', mix(well, fg, 0.45)),
    danger: on('danger', ansi[1]!),
    warn: on('warn', ansi[3]!),
    ok: on('ok', ansi[2]!),
    info: on('info', ansi[4]!),
    accent,
  };
  const terminal: ThemeTerminal = {
    background,
    foreground: over(t.foreground, background) ?? fg,
    cursor: over(t.cursor, background) ?? fg,
    selection: over(t.selection, background) ?? mix(well, accent, 0.25),
    ansi,
  };
  return { name: p.name, base, ui, terminal };
}
