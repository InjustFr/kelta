import { describe, expect, it } from 'vitest';

import { percentile } from './bench';

describe('percentile', () => {
  it('uses the nearest rank', () => {
    const v = Array.from({ length: 100 }, (_, i) => 100 - i);
    expect(percentile(v, 95)).toBe(95);
    expect(percentile(v, 99)).toBe(99);
    expect(percentile([7], 95)).toBe(7);
    expect(percentile([], 95)).toBeNaN();
  });
});
