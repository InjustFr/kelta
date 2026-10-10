import { describe, expect, it, vi } from 'vitest';

import type { ILink, Terminal } from '@xterm/xterm';

import { candidates, fileLinkProvider, parseFileRefs } from './filelinks';

const FIXTURES: [string, { text: string; path: string; line: number }[]][] = [
  // rustc / cargo
  ['  --> src/limiter.rs:42:5', [{ text: 'src/limiter.rs:42:5', path: 'src/limiter.rs', line: 42 }]],
  ['error at a.rs:42:5.', [{ text: 'a.rs:42:5', path: 'a.rs', line: 42 }]],
  // tsc
  ['src/a.ts(42,5): error TS2322', [{ text: 'src/a.ts(42,5)', path: 'src/a.ts', line: 42 }]],
  ['ui/b.tsx(7)', [{ text: 'ui/b.tsx(7)', path: 'ui/b.tsx', line: 7 }]],
  // plain, Claude prose, absolute and relative prefixes
  ['see a.py:42', [{ text: 'a.py:42', path: 'a.py', line: 42 }]],
  [
    'Edited (crates/kelta-work/src/ops.rs:908) and `x.svelte:3`',
    [
      { text: 'crates/kelta-work/src/ops.rs:908', path: 'crates/kelta-work/src/ops.rs', line: 908 },
      { text: 'x.svelte:3', path: 'x.svelte', line: 3 },
    ],
  ],
  ['/Users/me/p/main.go:10:2', [{ text: '/Users/me/p/main.go:10:2', path: '/Users/me/p/main.go', line: 10 }]],
  [
    './lib/x.test.ts:1 ../y.rb:2',
    [
      { text: './lib/x.test.ts:1', path: './lib/x.test.ts', line: 1 },
      { text: '../y.rb:2', path: '../y.rb', line: 2 },
    ],
  ],
  ['@scope/pkg/index.js:3', [{ text: '@scope/pkg/index.js:3', path: '@scope/pkg/index.js', line: 3 }]],
  // no link: no line, no extension, ports, times, URLs
  ['src/limiter.rs', []],
  ['Makefile:12', []],
  ['localhost:8080 at 12:30:45', []],
  ['https://example.com/a.rs:42', []],
];

describe('parseFileRefs', () => {
  it.each(FIXTURES)('%s', (input, expected) => {
    expect(parseFileRefs(input).map(({ text, path, line }) => ({ text, path, line }))).toEqual(expected);
  });

  it('reports the column where the match starts', () => {
    expect(parseFileRefs('  --> src/a.rs:1')[0]!.index).toBe(6);
  });
});

describe('candidates', () => {
  it('keeps absolute paths and tries each root for relative ones, cwd first', () => {
    expect(candidates('/a/b.rs', ['/w'])).toEqual(['/a/b.rs']);
    expect(candidates('./src/a.rs', ['/w/sub/', '/w'])).toEqual(['/w/sub/src/a.rs', '/w/src/a.rs']);
    expect(candidates('a.rs', ['/w', '/w'])).toEqual(['/w/a.rs']);
  });
});

describe('fileLinkProvider', () => {
  const line = 'see src/a.rs:4:2 and gone.rs:9';
  const term = {
    buffer: { active: { getLine: () => ({ translateToString: () => line }) } },
    onWriteParsed: () => ({ dispose() {} }),
  } as unknown as Terminal;
  const links = (provider: ReturnType<typeof fileLinkProvider>) =>
    new Promise<ILink[] | undefined>((resolve) => provider.provideLinks(3, resolve));

  it('links only existing files, asks once per line, and opens the resolved path', async () => {
    const exists = vi.fn(async (paths: string[]) => paths.map((p) => p === '/w/src/a.rs'));
    const open = vi.fn();
    const provider = fileLinkProvider(term, { roots: () => ['/cwd', '/w'], exists, open });
    const found = (await links(provider))!;
    expect(exists).toHaveBeenCalledWith(['/cwd/src/a.rs', '/w/src/a.rs', '/cwd/gone.rs', '/w/gone.rs']);
    expect(found.map((l) => [l.text, l.range])).toEqual([
      ['src/a.rs:4:2', { start: { x: 5, y: 3 }, end: { x: 16, y: 3 } }],
    ]);
    found[0]!.activate(new MouseEvent('click'), found[0]!.text);
    expect(open).toHaveBeenCalledWith(expect.any(MouseEvent), '/w/src/a.rs', 4);
    await links(provider);
    expect(exists).toHaveBeenCalledTimes(1); // cached until new output
  });
});
