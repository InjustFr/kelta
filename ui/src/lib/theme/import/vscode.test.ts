import { describe, expect, it } from 'vitest';

import dracula from './fixtures/dracula.json?raw';
import githubLight from './fixtures/github-light.jsonc?raw';
import noTerminal from './fixtures/no-terminal.json?raw';
import { mix } from './derive';
import { fromVscode, fromVscodeColors, stripJsonc, themeId } from './vscode';

const HEX = /^#[0-9a-f]{6}$/;

describe('fromVscode', () => {
  it('converts Dracula (comments, trailing commas, #rrggbbaa)', () => {
    expect(fromVscode(dracula, 'dracula.json')).toEqual({
      name: 'Dracula',
      base: 'dark',
      ui: {
        bezel: '#21222c',
        bezel_raised: '#21222c',
        well: '#282a36',
        bg_float: '#21222c',
        fg: '#f8f8f2',
        fg_muted: '#c4c5c3',
        fg_subtle: '#6272a4',
        fg_chrome: '#f8f8f2',
        border: '#574b73', // #BD93F950 over the well
        border_strong: '#191a21',
        danger: '#ff5555',
        warn: '#ffb86c',
        ok: '#50fa7b',
        info: '#bd93f9',
        accent: '#6272a4',
      },
      terminal: {
        background: '#282a36',
        foreground: '#f8f8f2',
        cursor: '#f8f8f2',
        selection: '#44475a',
        ansi: [
          '#21222c',
          '#ff5555',
          '#50fa7b',
          '#f1fa8c',
          '#bd93f9',
          '#ff79c6',
          '#8be9fd',
          '#f8f8f2',
          '#6272a4',
          '#ff6e6e',
          '#69ff94',
          '#ffffa5',
          '#d6acff',
          '#ff92df',
          '#a4ffff',
          '#ffffff',
        ],
      },
    });
  });

  it('converts GitHub Light, terminal falling back to editor keys', () => {
    const def = fromVscode(githubLight, 'github-light.jsonc');
    if (def instanceof Error) throw def;
    expect(def.base).toBe('light');
    expect(def.ui).toMatchObject({
      well: '#ffffff',
      bezel: '#f6f8fa',
      fg_muted: '#656d76',
      accent: '#0969da',
    });
    expect(def.terminal).toMatchObject({
      background: '#ffffff', // derived from the well
      cursor: '#0969da', // editorCursor.foreground
      selection: '#cee1f8', // #0969da33 over white
    });
    expect(def.terminal.ansi?.[11]).toBe('#633c01');
  });

  it('derives the base, the name, the chrome and the whole terminal when keys are missing', () => {
    expect(fromVscode(noTerminal, 'solarized-light.json')).toEqual({
      name: 'Solarized Light',
      base: 'light',
      ui: {
        bezel: '#f2ebd8',
        bezel_raised: mix('#fdf6e3', '#657b83', 0.1),
        well: '#fdf6e3',
        bg_float: mix('#fdf6e3', '#657b83', 0.1),
        fg: '#657b83',
        fg_muted: mix('#657b83', '#fdf6e3', 0.25),
        fg_subtle: mix('#657b83', '#fdf6e3', 0.38),
        fg_chrome: mix('#657b83', '#fdf6e3', 0.25),
        border: mix('#fdf6e3', '#657b83', 0.15),
        border_strong: mix('#fdf6e3', '#657b83', 0.45),
        danger: '#b8352e',
        warn: '#8a6400',
        ok: '#2f7a47',
        info: '#2e62a8',
        accent: '#2aa198',
      },
      terminal: {
        background: '#fdf6e3',
        foreground: '#657b83',
        cursor: '#657b83',
        selection: mix('#fdf6e3', '#2aa198', 0.25),
        ansi: [
          '#1b2026',
          '#b8352e',
          '#2f7a47',
          '#8a6400',
          '#2e62a8',
          '#87459e',
          '#1d7a80',
          '#b9c0c7',
          '#5f6873',
          '#d24a42',
          '#3e9259',
          '#a87b00',
          '#3f78c2',
          '#a05cb8',
          '#2a9299',
          '#858e98',
        ],
      },
    });
  });

  it('reports include, invalid JSON, a missing colors object and no usable colours', () => {
    const msg = (text: string) => (fromVscode(text, 'x.json') as Error).message;
    expect(msg('{"include": "./base.json", "colors": {}}')).toBe(
      "This theme extends ./base.json. In VS Code run 'Developer: Generate Color Theme From Current Settings' and import that file.",
    );
    expect(msg('{"colors": ')).toBe('This file is not valid JSON');
    expect(msg('[1]')).toBe('No "colors" object in this theme');
    expect(msg('{"name": "x"}')).toBe('No "colors" object in this theme');
    expect(msg('{"colors": {"editor.background": "red", "foo": "#ffffff"}}')).toBe(
      'No usable colours in this theme',
    );
  });
});

describe('fromVscodeColors', () => {
  const base = (type: unknown, well = '#ffffff') =>
    (fromVscodeColors({ 'editor.background': well }, 'X', type) as { base: string }).base;

  it('takes the base from type (hc is dark), else from the luminance of the well', () => {
    expect(base('hc')).toBe('dark');
    expect(base('hcDark')).toBe('dark');
    expect(base('hcLight', '#000000')).toBe('light');
    expect(base(undefined, '#ffffff')).toBe('light');
    expect(base(undefined, '#3a3a3a')).toBe('dark');
    expect(base('weird', '#eeeeee')).toBe('light');
  });

  it('accepts every hex form, composites alpha, and writes only lowercase #rrggbb', () => {
    const def = fromVscodeColors(
      {
        'editor.background': '#000',
        'editor.foreground': '#FFFF', // #rgba, opaque
        'sideBar.background': '#ffffff80',
        'terminal.ansiRed': '#F00',
        'terminal.ansiGreen': '#0f08',
      },
      'X',
    );
    if (def instanceof Error) throw def;
    expect(def.ui.fg).toBe('#ffffff');
    expect(def.ui.bezel).toBe('#808080');
    expect(def.terminal.ansi?.slice(1, 3)).toEqual(['#ff0000', '#008800']);
    for (const v of [...Object.values(def.ui), ...Object.values(def.terminal).flat()]) expect(v).toMatch(HEX);
    expect(def.terminal.ansi).toHaveLength(16);
  });
});

describe('themeId', () => {
  it('slugs the name, avoids built-ins and always yields a valid id', () => {
    expect(themeId('GitHub Light Default', 'x.json')).toBe('github-light-default');
    expect(themeId('Crème Brûlée 2.0', 'x.json')).toBe('creme-brulee-2-0');
    expect(themeId('Bezel Dark', 'x.json')).toBe('bezel-dark-custom');
    expect(themeId('夜', 'night_owl.json')).toBe('night-owl');
    expect(themeId('夜', '夜.json')).toBe('imported');
  });
});

describe('stripJsonc', () => {
  it('drops comments and trailing commas but not string content', () => {
    const text = '{ "a": "http://x/*y*/", // c\n "b": [1, 2, ], /* d */ "c": "\\",", }';
    expect(JSON.parse(stripJsonc(text))).toEqual({ a: 'http://x/*y*/', b: [1, 2], c: '",' });
  });
});
