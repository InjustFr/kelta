import { describe, expect, it } from 'vitest';

import type { Settings, ThemeDef } from '$lib/gen';
import { settingsDefault } from '$lib/gen/fixtures';
import { colors, terminalPalette } from '$lib/terminal/theme';

import { resolveTheme } from './index';

function withThemes(themes: Record<string, unknown>, dark = 'x', light = 'bezel-light'): Settings {
  return {
    ...settingsDefault,
    app: { ...settingsDefault.app, dark_theme: dark, light_theme: light },
    themes: themes as Record<string, ThemeDef>,
  };
}

const ansi = Array.from({ length: 16 }, (_, i) => `#0000${(i + 16).toString(16)}`);

describe('resolveTheme', () => {
  it('defaults to Bezel with no overrides and the base palette', () => {
    for (const mode of ['dark', 'light'] as const) {
      const r = resolveTheme(settingsDefault, mode);
      expect(r.id).toBe(`bezel-${mode}`);
      expect(r.vars).toEqual({});
      expect(r.accent).toBeNull();
      expect(terminalPalette(r.terminal)).toEqual(terminalPalette(mode));
    }
    expect(resolveTheme(null, 'dark').id).toBe('bezel-dark');
  });

  it('applies a partial theme, lowercased, with terminal fallbacks to ui.well / ui.fg', () => {
    const r = resolveTheme(
      withThemes({
        x: {
          name: 'X',
          base: 'dark',
          ui: { well: '#002B36', fg: '#93a1a1', bezel_raised: '#073642', accent: '#268BD2', danger: 'red' },
          terminal: { cursor: '#ABCDEF', ansi },
        },
      }),
      'dark',
    );
    expect(r.id).toBe('x');
    expect(r.vars).toEqual({
      '--k-well': '#002b36',
      '--k-fg': '#93a1a1',
      '--k-bezel-raised': '#073642',
      '--k-term-bg': '#002b36',
      '--k-term-fg': '#93a1a1',
      '--k-term-cursor': '#abcdef',
    });
    expect(r.accent).toBe('#268bd2');
    expect(r.terminal).toMatchObject({ background: '#002b36', foreground: '#93a1a1', cursor: '#abcdef' });
    expect(r.terminal.selection).toBe(colors('dark').selection);
    expect(r.terminal.ansi).toEqual(ansi);
  });

  it('prefers terminal colours over ui ones and falls back per token on bad values', () => {
    const bad = [...ansi];
    bad[3] = 'yellow';
    const r = resolveTheme(
      withThemes({
        x: {
          name: 'X',
          base: 'dark',
          ui: { well: '#111111' },
          terminal: { background: '#222222', ansi: bad },
        },
      }),
      'dark',
    );
    expect(r.vars['--k-well']).toBe('#111111');
    expect(r.terminal.background).toBe('#222222');
    expect(r.terminal.ansi[3]).toBe(terminalPalette('dark').ansi[3]);
    expect(r.terminal.ansi[4]).toBe(ansi[4]);
    const short = resolveTheme(
      withThemes({ x: { name: 'X', base: 'dark', ui: {}, terminal: { ansi: ansi.slice(1) } } }),
      'dark',
    );
    expect(short.terminal.ansi).toEqual(terminalPalette('dark').ansi);
  });

  it('falls back to Bezel for a missing id or a theme of the other base', () => {
    expect(resolveTheme(withThemes({}), 'dark').id).toBe('bezel-dark');
    const light = withThemes({ x: { name: 'X', base: 'light', ui: { fg: '#000000' }, terminal: {} } });
    const r = resolveTheme(light, 'dark');
    expect(r.id).toBe('bezel-dark');
    expect(r.vars).toEqual({});
  });

  it('lets a user theme with a built-in id win', () => {
    const s = withThemes(
      { 'bezel-light': { name: 'Mine', base: 'light', ui: { bezel: '#eeeeee' }, terminal: {} } },
      'bezel-dark',
      'bezel-light',
    );
    const r = resolveTheme(s, 'light');
    expect(r.def.name).toBe('Mine');
    expect(r.vars).toEqual({ '--k-bezel': '#eeeeee' });
  });
});
