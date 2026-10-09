import { describe, expect, it } from 'vitest';

import { oklchChroma, projectAccent } from './accent';

describe('project accent', () => {
  it('measures oklch chroma', () => {
    expect(oklchChroma('#808080')).toBeLessThan(0.001);
    expect(oklchChroma('#fff')).toBeLessThan(0.001);
    expect(oklchChroma('#ff0000')).toBeCloseTo(0.2577, 3);
    expect(oklchChroma('red')).toBeNull();
  });

  it('keeps hued colours and drops greys', () => {
    expect(projectAccent('#3e7cb1')).toBe('#3e7cb1');
    expect(projectAccent('#5f6b7a')).toBeNull(); // slate swatch: chroma ~0.026
    expect(projectAccent('#000000')).toBeNull();
    expect(projectAccent(null)).toBeNull();
    expect(projectAccent('rebeccapurple')).toBe('rebeccapurple');
  });
});
