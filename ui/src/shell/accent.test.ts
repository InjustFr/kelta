import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';

import { oklch, projectAccent } from './accent';

describe('project accent', () => {
  // jsdom has no CSS.supports: accept hex and a few named colours.
  beforeAll(() => {
    vi.stubGlobal('CSS', {
      supports: (_p: string, v: string) =>
        /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(v) || ['red', 'rebeccapurple'].includes(v),
    });
  });
  afterAll(() => vi.unstubAllGlobals());

  it('measures oklch chroma and hue', () => {
    expect(oklch('#808080')!.c).toBeLessThan(0.001);
    expect(oklch('#fff')!.c).toBeLessThan(0.001);
    expect(oklch('#ff0000')!.c).toBeCloseTo(0.2577, 3);
    expect(oklch('#ff0000')!.h).toBeCloseTo(29.2, 0);
    expect(oklch('red')).toBeNull();
  });

  it('keeps hued colours and drops greys', () => {
    expect(projectAccent('#3e7cb1')).toBe('#3e7cb1');
    expect(projectAccent('#b88a2e')).toBe('#b88a2e'); // ochre: warm but clear of the alarm band
    expect(projectAccent('#5f6b7a')).toBeNull(); // slate swatch: chroma ~0.026
    expect(projectAccent('#000000')).toBeNull();
    expect(projectAccent(null)).toBeNull();
    expect(projectAccent('rebeccapurple')).toBe('rebeccapurple');
  });

  it('drops reds so a selection never reads as a needs-input lamp', () => {
    expect(projectAccent('#b5533c')).toBeNull(); // brick swatch
    expect(projectAccent('#b85278')).toBeNull(); // rose swatch
    expect(projectAccent('#c93a33')).toBeNull();
  });

  it('drops colours the browser cannot parse', () => {
    expect(projectAccent('bleu')).toBeNull();
    expect(projectAccent('#12345')).toBeNull();
  });
});
