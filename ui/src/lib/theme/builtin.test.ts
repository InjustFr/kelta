import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';

import type { ThemeDef, ThemeUi } from '$lib/gen';

import { projectAccent } from '../../shell/accent';
import { BUILTIN } from './builtin';
import { contrast } from './floor';
import { luminance } from './import/derive';

const TEXT = ['fg', 'fg_muted', 'fg_chrome', 'fg_subtle'] as const;
const SURFACES = ['well', 'bezel', 'bezel_raised', 'bg_float'] as const;
const STATUS = ['danger', 'warn', 'ok', 'info'] as const;

/** Every DESIGN §2.2 text pair below 4.5:1, as `token on surface`. */
function failures(ui: ThemeUi): string[] {
  const pairs = [
    ...TEXT.flatMap((t) => SURFACES.map((s) => [t, s] as const)),
    ...STATUS.flatMap((t) => (['well', 'bezel'] as const).map((s) => [t, s] as const)),
  ];
  return pairs.filter(([t, s]) => contrast(ui[t]!, ui[s]!) < 4.5).map(([t, s]) => `${t} on ${s}`);
}

const themes = Object.entries(BUILTIN).filter(([id]) => !id.startsWith('bezel-'));

describe('built-in themes', () => {
  it('ships the curated set in picker order', () => {
    expect(themes.map(([id, d]) => `${id}:${d.base}`)).toEqual([
      'solarized-dark:dark',
      'solarized-light:light',
      'gruvbox-dark:dark',
      'gruvbox-light:light',
      'nord:dark',
      'dracula:dark',
      'catppuccin-mocha:dark',
      'catppuccin-latte:light',
    ]);
  });

  it.each(themes)('%s sets every token as #rrggbb', (_id, def: ThemeDef) => {
    const hex = /^#[0-9a-f]{6}$/;
    expect(Object.values(def.ui)).toHaveLength(15);
    for (const v of Object.values(def.ui)) expect(v).toMatch(hex);
    const { ansi, ...rest } = def.terminal;
    for (const v of Object.values(rest)) expect(v).toMatch(hex);
    expect(ansi).toHaveLength(16);
    for (const v of ansi!) expect(v).toMatch(hex);
    expect(luminance(def.ui.well!) < 0.2).toBe(def.base === 'dark');
  });

  it.each(themes)('%s keeps text and status tokens at 4.5:1', (_id, def: ThemeDef) => {
    expect(failures(def.ui)).toEqual([]);
  });

  it('flags a text token lowered below 4.5:1', () => {
    const ui = BUILTIN.nord!.ui;
    expect(failures({ ...ui, fg_subtle: ui.border_strong })).toContain('fg_subtle on well');
  });

  describe('accent', () => {
    // jsdom has no CSS.supports.
    beforeAll(() => vi.stubGlobal('CSS', { supports: () => true }));
    afterAll(() => vi.unstubAllGlobals());

    it.each(themes)('%s accent survives projectAccent', (_id, def: ThemeDef) => {
      expect(projectAccent(def.ui.accent)).toBe(def.ui.accent);
    });
  });
});
