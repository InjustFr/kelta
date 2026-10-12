import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { settingsDefault } from '$lib/gen/fixtures';

import { BUILTIN } from './builtin';
import { BEZEL_UI, contrast, floor, type FloorUi } from './floor';
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
