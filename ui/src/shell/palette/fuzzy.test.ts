import { describe, expect, it } from 'vitest';

import { fuzzyScore, rank } from './fuzzy';

describe('fuzzyScore', () => {
  it('matches subsequences case-insensitively and rejects non-matches', () => {
    expect(fuzzyScore('npn', 'New pane')).not.toBeNull();
    expect(fuzzyScore('PAL', 'Command palette')).not.toBeNull();
    expect(fuzzyScore('xyz', 'Command palette')).toBeNull();
    expect(fuzzyScore('', 'anything')).toBe(0);
  });

  it('requires every token', () => {
    expect(fuzzyScore('shop claude', 'claude — Shop (working)')).not.toBeNull();
    expect(fuzzyScore('shop zzz', 'claude — Shop (working)')).toBeNull();
  });

  it('prefers prefixes and word starts over scattered matches', () => {
    const prefix = fuzzyScore('term', 'Terminal search')!;
    const word = fuzzyScore('term', 'Search terminal')!;
    const scattered = fuzzyScore('term', 'Tab emulation rendering m')!;
    expect(prefix).toBeGreaterThan(word);
    expect(word).toBeGreaterThan(scattered);
  });
});

describe('rank', () => {
  const items = ['Open settings', 'Open tickets', 'Switch project', 'New session', 'Next tab'];

  it('keeps the order for an empty query and honours the limit', () => {
    expect(rank(items, '', (s) => s, 3)).toEqual(items.slice(0, 3));
  });

  it('sorts by score and drops non-matches', () => {
    expect(rank(items, 'tick', (s) => s)[0]).toBe('Open tickets');
    expect(rank(items, 'ns', (s) => s)).toContain('New session');
    expect(rank(items, 'qqq', (s) => s)).toEqual([]);
  });

  it('is stable for equal scores', () => {
    expect(rank(['b one', 'a one'], 'one', (s) => s)).toEqual(['b one', 'a one']);
  });
});
