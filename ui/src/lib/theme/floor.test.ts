import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { settingsDefault } from '$lib/gen/fixtures';

import { BUILTIN } from './builtin';
import { BEZEL_UI, contrast, floor, type FloorUi } from './floor';
import { oklab, parseHex } from './hex';
import { resolveTheme } from './index';

const SURFACES = ['well', 'bezel', 'bezel_raised', 'bg_float'] as const;

// An imported Arctis Moon (#387): rail, toolbars, menus and wells one plane, faint subtle text.
const moon: FloorUi = {
  bezel: '#212337',
  bezel_raised: '#212337',
  well: '#212337',
  bg_float: '#212337',
  fg: '#d8dbf3',
  fg_muted: '#a9acc9',
  fg_subtle: '#5b5e75',
  fg_chrome: '#a9acc9',
  border: '#323453',
  border_strong: '#323453',
};

describe('contrast floor', () => {
  it('BEZEL_UI mirrors tokens.css (light block, then the two dark blocks)', () => {
    const css = readFileSync('src/styles/tokens.css', 'utf8'); // vitest runs from ui/
    for (const key of Object.keys(BEZEL_UI.dark) as (keyof FloorUi)[]) {
      const name = key.replace(/_/g, '-');
      const values = [...css.matchAll(new RegExp(`--k-${name}: (#[0-9a-f]{6})`, 'g'))].map((m) => m[1]);
      expect(values, name).toEqual([BEZEL_UI.light[key], BEZEL_UI.dark[key], BEZEL_UI.dark[key]]);
    }
  });

  it.each(Object.entries(BUILTIN))('%s needs no fix', (_id, def) => {
    const ui = Object.fromEntries(Object.entries(def.ui).filter(([, v]) => v));
    expect(floor({ ...BEZEL_UI[def.base], ...ui })).toEqual({ fixed: {}, notes: [] });
  });

  it('derives collapsed surface steps and lifts subtle text and the strong border', () => {
    const { fixed, notes } = floor(moon);
    const ui = { ...moon, ...fixed };
    expect(Object.keys(fixed).sort()).toEqual([
      'bezel',
      'bezel_raised',
      'bg_float',
      'border_strong',
      'fg_subtle',
    ]);
    expect(new Set(SURFACES.map((s) => ui[s])).size).toBe(3); // well, bezel, raised = float
    for (const s of SURFACES) expect(contrast(ui.fg_subtle, ui[s])).toBeGreaterThanOrEqual(4.5);
    for (const s of ['well', 'bezel'] as const) {
      expect(contrast(ui.border_strong, ui[s])).toBeGreaterThanOrEqual(3);
      expect(contrast(ui.border_strong, ui[s])).toBeGreaterThan(contrast(ui.border, ui[s]));
    }
    expect(notes).toEqual([
      'Surface steps derived from the background',
      'Subtle text raised to 4.5:1',
      'Strong border raised to 3:1',
    ]);
  });

  it('flags a theme whose fg itself is below the floor', () => {
    const { notes } = floor({ ...moon, fg: '#55587a' });
    expect(notes).toContain('Some text or borders stay below the contrast floor');
  });

  it('resolveTheme applies the fixes and keeps Bezel clean', () => {
    const s = {
      ...settingsDefault,
      app: { ...settingsDefault.app, dark_theme: 'moon' },
      themes: { moon: { name: 'Moon', base: 'dark' as const, ui: moon, terminal: {} } },
    };
    const r = resolveTheme(s as unknown as typeof settingsDefault, 'dark');
    const { fixed, notes } = floor(moon);
    expect(r.vars).toMatchObject({
      '--k-bezel': fixed.bezel,
      '--k-fg-subtle': fixed.fg_subtle,
      '--k-well': moon.well,
    });
    expect(r.notes).toEqual(notes);
    expect(resolveTheme(settingsDefault, 'dark').notes).toEqual([]);
  });
});

// oklab [L, a, b] back to #rrggbb, clipped to sRGB.
function hexOf([L, a, b]: number[]): string {
  const [l, m, s] = [
    L! + 0.3963377774 * a! + 0.2158037573 * b!,
    L! - 0.1055613458 * a! - 0.0638541728 * b!,
    L! - 0.0894841775 * a! - 1.291485548 * b!,
  ].map((v) => v ** 3);
  const lin = [
    4.0767416621 * l! - 3.3077115913 * m! + 0.2309699292 * s!,
    -1.2684380046 * l! + 2.6097574011 * m! - 0.3413193965 * s!,
    -0.0041960863 * l! - 0.7034186147 * m! + 1.707614701 * s!,
  ];
  return (
    '#' +
    lin
      .map((v) =>
        Math.round(
          255 * (v <= 0.0031308 ? 12.92 * v : 1.055 * Math.max(0, Math.min(1, v)) ** (1 / 2.4) - 0.055),
        )
          .toString(16)
          .padStart(2, '0'),
      )
      .join('')
  );
}
const lerp = (x: number[], y: number[], p: number) => x.map((v, i) => v + (y[i]! - v) * p);

// tokens.css: --k-bg-selected-row mixes the accent into the well (dark 28% of L .74 C .09, light 14% of L .5 C .11)
// and rows take --k-fg-subtle-sel / --k-fg-muted-sel, 60% of the way from subtle / muted to fg.
// Every other consumer keeps the plain token on --k-bg-selected (dark 16%) with no remap; #381 only moved the
// dark mix, so the plain sweep skips light themes (their 14% mix is unchanged and not held to 4.5:1 unmapped).
describe.each([
  ['row (remapped)', 0.6, { dark: 0.28, light: 0.14 }],
  ['plain token', 0, { dark: 0.16, light: 0.14 }],
] as const)('%s', (_name, remap, shares) => {
  describe.each(['fg_subtle', 'fg_muted'] as const)('%s text on a selected surface', (token) => {
    it.each(Object.entries(BUILTIN).filter(([, d]) => remap || d.base === 'dark'))(
      '%s keeps 4.5:1 at every project hue',
      (_id, def) => {
        const ui = {
          ...BEZEL_UI[def.base],
          ...Object.fromEntries(Object.entries(def.ui).filter(([, v]) => v)),
        } as FloorUi;
        const [L, C] = def.base === 'dark' ? [0.74, 0.09] : [0.5, 0.11];
        const share = shares[def.base];
        const [well, fg, from] = [ui.well, ui.fg, ui[token]].map((h) => oklab(parseHex(h)!));
        const text = hexOf(lerp(from!, fg!, remap));
        for (let h = 0; h < 360; h += 5) {
          const rad = (h * Math.PI) / 180;
          const selected = hexOf(lerp(well!, [L!, C! * Math.cos(rad), C! * Math.sin(rad)], share));
          expect(contrast(text, selected), `hue ${h}`).toBeGreaterThanOrEqual(4.5);
        }
      },
    );
  });
});
