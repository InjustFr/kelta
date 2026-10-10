// Terminal file links (`src/a.rs:42:5`, `a.ts(42,5)`, `a.py:42`): parsed per buffer line, kept only
// when the file exists (one batched `fs_exists` per hovered line, cached until new output).
// shortcut: the column is matched but not used (`editor_open` takes a line); pass it once the
// editor contract carries one.

import type { IBufferRange, ILink, ILinkProvider, Terminal } from '@xterm/xterm';

export interface FileRef {
  /** 0-based index of the match in the line. */
  index: number;
  text: string;
  path: string;
  line: number;
}

// shortcut: the file name needs an extension (no `Makefile:3`), so ports and times never match.
const FILE_RE =
  /(?<![\w./@+-])((?:\.{1,2}\/|\/)?(?:[\w.@+-]+\/)*[\w@+-][\w.@+-]*\.[A-Za-z]\w*)(?::(\d+)(?::\d+)?|\((\d+)(?:,\s*\d+)?\))/g;

export function parseFileRefs(text: string): FileRef[] {
  return [...text.matchAll(FILE_RE)].map((m) => ({
    index: m.index,
    text: m[0],
    path: m[1]!,
    line: Number(m[2] ?? m[3]),
  }));
}

/** Absolute candidates of a path: itself when absolute, else under each root (cwd first). */
export function candidates(path: string, roots: readonly string[]): string[] {
  if (path.startsWith('/')) return [path];
  const rel = path.replace(/^\.\//, '');
  return [...new Set(roots.map((r) => `${r.replace(/\/+$/, '')}/${rel}`))];
}

export interface FileLinkDeps {
  /** Directories relative paths resolve against, in order (session cwd, then worktree root). */
  roots: () => string[];
  exists: (paths: string[]) => Promise<boolean[]>;
  open: (event: MouseEvent, path: string, line: number) => void;
}

export function fileLinkProvider(term: Terminal, deps: FileLinkDeps): ILinkProvider & { dispose(): void } {
  const known = new Map<string, boolean>();
  const sub = term.onWriteParsed(() => known.clear());
  return {
    dispose: () => sub.dispose(),
    provideLinks(y, callback) {
      // shortcut: one buffer row, so a path wrapped onto the next row or after wide chars is not linked.
      const refs = parseFileRefs(term.buffer.active.getLine(y - 1)?.translateToString(true) ?? '');
      if (refs.length === 0) return callback(undefined);
      const roots = deps.roots();
      const cands = refs.map((r) => candidates(r.path, roots));
      const ask = [...new Set(cands.flat())].filter((p) => !known.has(p));
      const ready = ask.length > 0 ? deps.exists(ask) : Promise.resolve([]);
      ready.then(
        (ok) => {
          ask.forEach((p, i) => known.set(p, ok[i] === true));
          const links = refs.flatMap((r, i): ILink[] => {
            const file = cands[i]!.find((p) => known.get(p));
            if (!file) return [];
            const range: IBufferRange = {
              start: { x: r.index + 1, y },
              end: { x: r.index + r.text.length, y },
            };
            return [{ range, text: r.text, activate: (e) => deps.open(e, file, r.line) }];
          });
          callback(links.length > 0 ? links : undefined);
        },
        () => callback(undefined),
      );
    },
  };
}
