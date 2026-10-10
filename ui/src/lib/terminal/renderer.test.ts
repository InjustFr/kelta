import { readFileSync } from 'node:fs';

import { afterEach, describe, expect, it } from 'vitest';

import {
  acquireWebglContext,
  activeWebglContexts,
  chooseRenderer,
  MAX_WEBGL_CONTEXTS,
  probeVerdict,
  releaseWebglContext,
  resetWebglContextsForTests,
} from './renderer';
import { terminalPalette, xtermTheme } from './theme';

afterEach(() => resetWebglContextsForTests());

describe('chooseRenderer', () => {
  it('auto = WebGL on macOS, DOM on Linux', () => {
    expect(chooseRenderer('auto', 'macos', true, 0)).toBe('webgl');
    expect(chooseRenderer('auto', 'linux', true, 0)).toBe('dom');
  });

  it('explicit settings win when WebGL is available', () => {
    expect(chooseRenderer('webgl', 'linux', true, 0)).toBe('webgl');
    expect(chooseRenderer('dom', 'macos', true, 0)).toBe('dom');
  });

  it('falls back to DOM without WebGL support or beyond the 8-context cap', () => {
    expect(chooseRenderer('webgl', 'macos', false, 0)).toBe('dom');
    expect(chooseRenderer('webgl', 'macos', true, MAX_WEBGL_CONTEXTS)).toBe('dom');
    expect(chooseRenderer('webgl', 'macos', true, MAX_WEBGL_CONTEXTS - 1)).toBe('webgl');
  });
});

describe('WebGL context accounting', () => {
  it('caps concurrent contexts at 8 and frees slots on release (context loss)', () => {
    for (let i = 0; i < MAX_WEBGL_CONTEXTS; i++) expect(acquireWebglContext()).toBe(true);
    expect(acquireWebglContext()).toBe(false);
    expect(activeWebglContexts()).toBe(8);
    releaseWebglContext();
    expect(acquireWebglContext()).toBe(true);
    resetWebglContextsForTests();
    releaseWebglContext(); // never negative
    expect(activeWebglContexts()).toBe(0);
  });
});

describe('probeVerdict', () => {
  it('suggests WebGL only when it is smooth and DOM is not', () => {
    expect(probeVerdict(60, 30)).toBe('suggest-webgl');
    expect(probeVerdict(60, 50)).toBe('keep-dom');
    expect(probeVerdict(40, 20)).toBe('keep-dom');
  });
});

describe('themes', () => {
  it('push a complete palette for both modes', () => {
    for (const mode of ['dark', 'light'] as const) {
      const p = terminalPalette(mode);
      expect(p.ansi).toHaveLength(16);
      for (const c of [p.foreground, p.background, p.cursor, ...p.ansi]) expect(c).toMatch(/^#[0-9a-f]{6}$/);
      expect(xtermTheme(mode).background).toBe(p.background);
      expect(xtermTheme(mode).brightWhite).toBe(p.ansi[15]);
    }
    expect(terminalPalette('dark').background).not.toBe(terminalPalette('light').background);
  });

  it('tokens.css mirrors theme.ts (light block first, then the two dark blocks)', () => {
    const tokensCss = readFileSync('src/styles/tokens.css', 'utf8'); // vitest runs from ui/
    const values = (name: string) =>
      [...tokensCss.matchAll(new RegExp(`--k-term-${name}: (#[0-9a-f]{6})`, 'g'))].map((m) => m[1]);
    for (const [name, key] of [
      ['bg', 'background'],
      ['fg', 'foreground'],
      ['cursor', 'cursor'],
      ['selection', 'selectionBackground'],
    ] as const) {
      expect(values(name)).toEqual([
        xtermTheme('light')[key],
        xtermTheme('dark')[key],
        xtermTheme('dark')[key],
      ]);
    }
  });
});
