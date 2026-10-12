// DESIGN §2.2 for every theme, not only the built-ins (#387). Imported themes often paint the rail,
// toolbars and wells one colour and keep a faint "subtle" text: resolveTheme re-derives the collapsed
// surface steps, lifts text toward fg to 4.5:1 and border_strong to 3:1, and names each fix for the picker.

import type { ThemeUi } from '$lib/gen';
import { msg } from '$lib/i18n/index.svelte';
import type { ThemeMode } from '$lib/terminal/theme';

import { oklab, parseHex } from './hex';
import { luminance, mix, surfaces } from './import/derive';

/** The surface, text and border tokens of a theme, all set. */
export type FloorUi = Record<Exclude<keyof ThemeUi, 'danger' | 'warn' | 'ok' | 'info' | 'accent'>, string>;

/** Bezel's values from tokens.css (floor.test.ts checks they match), under a partial theme. */
export const BEZEL_UI: Record<ThemeMode, FloorUi> = {
  light: {
    bezel: '#e1e5e9',
    bezel_raised: '#eaedf0',
    well: '#fafbfb',
    bg_float: '#ffffff',
    fg: '#161b21',
    fg_muted: '#46505b',
    fg_subtle: '#535d68',
    fg_chrome: '#434d57',
    border: '#cbd1d7',
    border_strong: '#77818c',
  },
  dark: {
    bezel: '#1a1e24',
    bezel_raised: '#22272e',
    well: '#121519',
    bg_float: '#282e36',
    fg: '#e1e6ec',
    fg_muted: '#aab4bf',
    fg_subtle: '#97a1ad',
    fg_chrome: '#a6b0bb',
    border: '#2d343d',
    border_strong: '#6a7582',
  },
};

export function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi! + 0.05) / (lo! + 0.05);
}

function distance(a: string, b: string): number {
  const [x, y] = [a, b].map((h) => oklab(parseHex(h)!));
  return Math.hypot(...x!.map((v, i) => v - y![i]!));
}

// Below the closest built-in step (Gruvbox Light's raised, 0.019); a light float may sit on the well.
const COLLAPSED = 0.015;
const SURFACES = ['well', 'bezel', 'bezel_raised', 'bg_float'] as const;
const LIFTS = [
  ['fg_muted', 4.5, SURFACES, msg('Muted text raised to 4.5:1')],
  ['fg_subtle', 4.5, SURFACES, msg('Subtle text raised to 4.5:1')],
  ['fg_chrome', 4.5, SURFACES, msg('Chrome text raised to 4.5:1')],
  // Inputs and the gutter grip sit on the well and the bezel.
  ['border_strong', 3, ['well', 'bezel'], msg('Strong border raised to 3:1')],
] as const;

/** The tokens `ui` must change to meet the floor, and the English notes saying why. */
export function floor(ui: FloorUi): { fixed: Partial<FloorUi>; notes: string[] } {
  const fixed: Partial<FloorUi> = {};
  const notes: string[] = [];
  const out = { ...ui };
  const set = (key: keyof FloorUi, v: string) => (out[key] = fixed[key] = v);

  const near = (k: keyof FloorUi) => distance(ui[k], ui.well) < COLLAPSED;
  if (near('bezel') && near('bezel_raised')) {
    const step = surfaces(ui.well, ui.fg);
    set('bezel', step.bezel);
    set('bezel_raised', step.bezel_raised);
    if (near('bg_float')) set('bg_float', step.bezel_raised);
    notes.push(msg('Surface steps derived from the background'));
  }

  let short = SURFACES.some((s) => contrast(out.fg, out[s]) < 4.5);
  for (const [key, min, on, note] of LIFTS) {
    // border_strong also stays stronger than border on the same surface.
    const ok = (c: string) =>
      on.every(
        (s) =>
          contrast(c, out[s]) >= min &&
          (key !== 'border_strong' || contrast(c, out[s]) > contrast(out.border, out[s])),
      );
    if (ok(out[key])) continue;
    let p = 0;
    while (p < 1 && !ok(mix(out[key], out.fg, p))) p = Math.min(1, p + 0.05);
    const lifted = mix(out[key], out.fg, p);
    set(key, lifted);
    if (ok(lifted)) notes.push(note);
    else short = true;
  }
  if (short) notes.push(msg('Some text or borders stay below the contrast floor'));
  return { fixed, notes };
}
