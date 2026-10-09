import { describe, expect, it } from 'vitest';

import { needsPasteConfirmation, sanitizePaste } from './paste';

describe('sanitizePaste', () => {
  it('strips bracketed paste markers so pasted text cannot end the paste early', () => {
    expect(sanitizePaste('a\x1b[201~rm -rf /\x1b[200~b')).toBe('arm -rf /b');
    expect(sanitizePaste('plain')).toBe('plain');
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
