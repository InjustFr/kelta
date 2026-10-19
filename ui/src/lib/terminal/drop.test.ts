import { describe, expect, it } from 'vitest';

import { shellEscape } from './drop';

describe('shellEscape', () => {
  it('leaves a plain path alone', () => {
    expect(shellEscape('/home/me/notes-1.2_final.txt')).toBe('/home/me/notes-1.2_final.txt');
    expect(shellEscape('/tmp/café 日本')).toBe('/tmp/café\\ 日本');
  });

  it('escapes spaces, quotes and every shell metacharacter', () => {
    expect(shellEscape('/tmp/my file.png')).toBe('/tmp/my\\ file.png');
    const special = ` \\'"()[]{}<>|;&*?$!#~\``;
    expect(shellEscape(special)).toBe([...special].map((c) => '\\' + c).join(''));
  });

  it('escapes a leading tilde so the shell does not expand it', () => {
    expect(shellEscape('~/x')).toBe('\\~/x');
  });
});
