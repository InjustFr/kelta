import { describe, expect, it } from 'vitest';

import { colors } from '$lib/terminal/theme';

import catppuccinLatte from './fixtures/Catppuccin Latte.toml?raw';
import dracula from './fixtures/Dracula.itermcolors?raw';
import gruvboxDark from './fixtures/Gruvbox Dark?raw';
import draculaVscode from './fixtures/dracula.json?raw';
import nord from './fixtures/Nord.conf?raw';
import oneLight from './fixtures/one-light.yaml?raw';
import { detect, fromThemeFile, UNRECOGNISED } from './terminal';

const load = (text: string, fileName: string) => {
  const def = fromThemeFile(text, fileName);
  if (def instanceof Error) throw def;
  return def;
};

// Upstream fixtures: iTerm2-Color-Schemes (MIT) for iTerm2/Ghostty/Kitty/Alacritty, tinted-theming/schemes (MIT).
describe('fromThemeFile', () => {
  it('detects each format by content, whatever the file name', () => {
    expect(detect(draculaVscode)).toBe('vscode');
    expect(detect(dracula)).toBe('iterm');
    expect(detect(gruvboxDark)).toBe('ghostty');
    expect(detect(nord)).toBe('kitty');
    expect(detect(catppuccinLatte)).toBe('alacritty');
    expect(detect(oneLight)).toBe('base16');
    expect(load(draculaVscode, 'x').ui.well).toBe('#282a36');
  });

  it('imports iTerm2 (plist float components)', () => {
    const def = load(dracula, 'Dracula.itermcolors');
    expect(def.name).toBe('Dracula');
    expect(def.base).toBe('dark');
    expect(def.terminal).toEqual({
      background: '#282a36',
      foreground: '#f8f8f2',
      cursor: '#f8f8f2',
      selection: '#44475a',
      ansi: [
        ...['#21222c', '#ff5555', '#50fa7b', '#f1fa8c', '#bd93f9', '#ff79c6', '#8be9fd', '#f8f8f2'],
        ...['#6272a4', '#ff6e6e', '#69ff94', '#ffffa5', '#d6acff', '#ff92df', '#a4ffff', '#ffffff'],
      ],
    });
    // Chrome derived from the terminal: well = background, accent = ANSI blue.
    expect(def.ui).toMatchObject({
      well: '#282a36',
      fg: '#f8f8f2',
      accent: '#bd93f9',
      danger: '#ff5555',
      ok: '#50fa7b',
    });
    expect(Object.values(def.ui)).toHaveLength(15);
  });

  it('imports a Ghostty theme with no extension', () => {
    const def = load(gruvboxDark, 'Gruvbox Dark');
    expect(def.name).toBe('Gruvbox Dark');
    expect(def.base).toBe('dark');
    expect(def.terminal).toEqual({
      background: '#282828',
      foreground: '#ebdbb2',
      cursor: '#ebdbb2',
      selection: '#665c54',
      ansi: [
        ...['#282828', '#cc241d', '#98971a', '#d79921', '#458588', '#b16286', '#689d6a', '#a89984'],
        ...['#928374', '#fb4934', '#b8bb26', '#fabd2f', '#83a598', '#d3869b', '#8ec07c', '#ebdbb2'],
      ],
    });
    expect(def.ui.accent).toBe('#458588');
  });

  it('imports Kitty', () => {
    const def = load(nord, 'Nord.conf');
    expect(def.name).toBe('Nord');
    expect(def.base).toBe('dark');
    expect(def.terminal).toEqual({
      background: '#2e3440',
      foreground: '#d8dee9',
      cursor: '#eceff4',
      selection: '#d8dee9',
      ansi: [
        ...['#3b4252', '#bf616a', '#a3be8c', '#ebcb8b', '#81a1c1', '#b48ead', '#88c0d0', '#e5e9f0'],
        ...['#596377', '#bf616a', '#a3be8c', '#ebcb8b', '#81a1c1', '#b48ead', '#8fbcbb', '#eceff4'],
      ],
    });
    expect(def.ui.well).toBe('#2e3440');
  });

  it('imports Alacritty TOML, base from the light background', () => {
    const def = load(catppuccinLatte, 'Catppuccin Latte.toml');
    expect(def.name).toBe('Catppuccin Latte');
    expect(def.base).toBe('light');
    expect(def.terminal).toEqual({
      background: '#eff1f5',
      foreground: '#4c4f69',
      cursor: '#dc8a78',
      selection: '#dc8a78',
      ansi: [
        ...['#bcc0cc', '#d20f39', '#40a02b', '#df8e1d', '#1e66f5', '#ea76cb', '#179299', '#5c5f77'],
        ...['#acb0be', '#e7103f', '#46b02f', '#e49931', '#3878f6', '#ef95d7', '#19a1a8', '#6c6f85'],
      ],
    });
    expect(def.ui).toMatchObject({ well: '#eff1f5', fg: '#4c4f69', accent: '#1e66f5' });
  });

  it('imports base16: chrome and ANSI slots as specified, name and variant from the file', () => {
    const def = load(oneLight, 'one-light.yaml');
    expect(def.name).toBe('One Light');
    expect(def.base).toBe('light');
    expect(def.ui).toMatchObject({
      well: '#fafafa',
      bezel_raised: '#f0f0f1',
      border: '#e5e5e6',
      fg_subtle: '#696c77',
      fg: '#383a42',
      accent: '#4078f2',
    });
    expect(def.terminal).toEqual({
      background: '#fafafa',
      foreground: '#383a42',
      cursor: '#383a42',
      selection: '#e5e5e6',
      ansi: [
        ...['#fafafa', '#ca1243', '#50a14f', '#c18401', '#4078f2', '#a626a4', '#0184bc', '#383a42'],
        ...['#a0a1a7', '#ca1243', '#50a14f', '#c18401', '#4078f2', '#a626a4', '#0184bc', '#090a0b'],
      ],
    });
    // variant wins over luminance; old `scheme:` names, flat keys without quotes or `#`.
    const old = 'scheme: "Old"\nvariant: dark\nbase00: fafafa # bg\nbase0D: 4078f2\n';
    expect(load(old, 'x.yaml')).toMatchObject({ name: 'Old', base: 'dark', ui: { accent: '#4078f2' } });
  });

  it('fills a short palette from Bezel and reads 0x / bare hex', () => {
    const def = load('palette = 1=ff0000\nbackground = 0x101010\n', 'my-theme');
    expect(def.name).toBe('My Theme');
    expect(def.terminal.ansi).toEqual(colors('dark').ansi.map((c, i) => (i === 1 ? '#ff0000' : c)));
    expect(def.terminal.background).toBe('#101010');
  });

  it('does not confuse Kitty and Ghostty', () => {
    expect(detect('background #000000\ncolor1 #ff0000')).toBe('kitty');
    expect(detect('background = #000000\npalette = 1=#ff0000')).toBe('ghostty');
    expect(detect('color1 = #ff0000')).toBeNull();
  });

  it('rejects unrecognised content', () => {
    for (const text of ['', 'hello', '<plist><dict/></plist>', 'background #000000']) {
      expect((fromThemeFile(text, 'x') as Error).message).toBe(UNRECOGNISED);
    }
  });
});
