import { describe, expect, it } from 'vitest';

import { needsPasteConfirmation, sanitizePaste } from './paste';

describe('sanitizePaste', () => {
  it('strips bracketed paste markers so pasted text cannot end the paste early', () => {
    expect(sanitizePaste('a\x1b[201~rm -rf /\x1b[200~b')).toBe('arm -rf /b');
    expect(sanitizePaste('plain')).toBe('plain');
  });

  it('strips nested and split markers until none remain', () => {
    expect(sanitizePaste('a\x1b[20\x1b[201~1~\nrm -rf ~\n')).toBe('a\nrm -rf ~\n');
    expect(sanitizePaste('\x1b[20\x1b[20\x1b[200~1~0~x')).toBe('x');
  });

  it('strips deeply nested markers in linear time', () => {
    // No absolute limit (load breaks those): 8x the input must cost ~8x (linear), not ~64x (quadratic).
    // The best of several runs drops load spikes; 24 sits between 8 and 64.
    const nested = (depth: number) => 'a' + '\x1b[2'.repeat(depth) + '00~'.repeat(depth) + 'b';
    const best = (input: string) => {
      let min = Infinity;
      for (let i = 0; i < 5; i++) {
        const start = performance.now();
        expect(sanitizePaste(input)).toBe('ab');
        min = Math.min(min, performance.now() - start);
      }
      return min;
    };
    best(nested(1_000)); // warm up the JIT
    expect(best(nested(80_000)) / best(nested(10_000))).toBeLessThan(24);
  });
});

describe('needsPasteConfirmation', () => {
  it('asks for multi-line text without bracketed paste', () => {
    expect(needsPasteConfirmation('a\nb', false, true)).toBe(true);
    expect(needsPasteConfirmation('a\r\nb\r\n', false, true)).toBe(true);
  });

  it('does not ask for a single line, with bracketed paste or when disabled', () => {
    expect(needsPasteConfirmation('one line', false, true)).toBe(false);
    expect(needsPasteConfirmation('one line\n', false, true)).toBe(false);
    expect(needsPasteConfirmation('a\nb', true, true)).toBe(false);
    expect(needsPasteConfirmation('a\nb', false, false)).toBe(false);
  });
});
