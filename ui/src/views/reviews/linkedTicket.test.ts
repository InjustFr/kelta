import { describe, expect, it } from 'vitest';

import { sameKey } from './linkedTicket';

describe('sameKey', () => {
  it('matches exact keys case-insensitively', () => {
    expect(sameKey('shop-1', 'SHOP-1', 'o/r')).toBe(true);
    expect(sameKey('SHOP-12', 'SHOP-1', 'o/r')).toBe(false);
  });
  it('matches #N against repo#N (forges) and the bare id (Redmine), not another repo', () => {
    expect(sameKey('o/r#12', '#12', 'o/r')).toBe(true);
    expect(sameKey('12', '#12', 'o/r')).toBe(true);
    expect(sameKey('o/other#12', '#12', 'o/r')).toBe(false);
  });
});
