import { describe, expect, it } from 'vitest';

import type { Settings } from '$lib/gen';
import { settingsDefault } from '$lib/gen/fixtures';

import { BUILTIN } from './builtin';
import { resolveTheme } from './index';
import { LEGACY_SWATCHES, parseProjectColor, resolveProjectColor, slotLabel, SLOTS } from './projectColor';

const themes = Object.entries(BUILTIN).map(([id, def]) => {
  const slot = def.base === 'dark' ? 'dark_theme' : 'light_theme';
  const settings: Settings = { ...settingsDefault, app: { ...settingsDefault.app, [slot]: id } };
  return resolveTheme(settings, def.base);
});

const LEGACY: [string, keyof typeof SLOTS][] = [
  ['#3e7cb1', 'blue'],
  ['#5e8c4a', 'green'],
  ['#b88a2e', 'yellow'],
  ['#b5533c', 'red'],
  ['#8a5a9e', 'magenta'],
  ['#2f8c86', 'cyan'],
  ['#b85278', 'bright-magenta'],
  ['#4b5fc4', 'bright-blue'],
  ['#85863a', 'bright-yellow'],
];

describe('resolveProjectColor', () => {
  it('covers every built-in theme', () => {
    expect(themes.map((t) => t.id).sort()).toEqual(Object.keys(BUILTIN).sort());
  });

  it.each(themes.map((t) => [t.id, t] as const))('%s: slots follow the theme', (_, theme) => {
    for (const [slot, i] of Object.entries(SLOTS)) {
      expect(resolveProjectColor(`theme:${slot}`, theme)).toBe(theme.terminal.ansi[i]);
    }
  });

  it.each(themes.map((t) => [t.id, t] as const))('%s: legacy swatches map to their slot', (_, theme) => {
    expect(Object.keys(LEGACY_SWATCHES)).toHaveLength(LEGACY.length);
    for (const [hex, slot] of LEGACY) {
      expect(resolveProjectColor(hex, theme)).toBe(theme.terminal.ansi[SLOTS[slot]]);
      expect(resolveProjectColor(hex.toUpperCase(), theme)).toBe(theme.terminal.ansi[SLOTS[slot]]);
    }
  });

  it.each(themes.map((t) => [t.id, t] as const))('%s: slate and custom hexes stay as typed', (_, theme) => {
    for (const hex of ['#5f6b7a', '#ff8800', '#FF8800']) expect(resolveProjectColor(hex, theme)).toBe(hex);
  });

  it('paints slots through --k-ansi-N without a theme, so a theme switch recolours with no JS', () => {
    for (const [slot, i] of Object.entries(SLOTS)) {
      expect(resolveProjectColor(`theme:${slot}`)).toBe(`var(--k-ansi-${i})`);
    }
    expect(resolveProjectColor('#3e7cb1')).toBe(`var(--k-ansi-${SLOTS.blue})`);
    expect(resolveProjectColor('#ff8800')).toBe('#ff8800');
  });

  it('reads #rgb as #rrggbb', () => {
    expect(resolveProjectColor('#abc', themes[0]!)).toBe('#aabbcc');
    expect(parseProjectColor('#F80')).toEqual({ hex: '#FF8800' });
  });

  it('rejects anything else', () => {
    for (const bad of [
      null,
      undefined,
      '',
      'green',
      'theme:black',
      'theme:',
      'theme:toString',
      '#f8',
      '#ff880',
      'rgb(1,2,3)',
    ]) {
      expect(parseProjectColor(bad)).toBeNull();
      expect(resolveProjectColor(bad, themes[0]!)).toBeNull();
    }
  });

  it('labels slots for tooltips', () => {
    expect(slotLabel('green')).toBe('Green');
    expect(slotLabel('bright-blue')).toBe('Bright blue');
  });
});
