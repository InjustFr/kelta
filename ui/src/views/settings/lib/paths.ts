// Dotted settings paths, matching kelta-config's `join_path` / `split_path`: a segment with
// anything but `[A-Za-z0-9_-]` is double-quoted (`keys.bindings."palette.open"`).

import type { JsonValue } from '$lib/gen';

const BARE = /^[A-Za-z0-9_-]+$/;

export function joinPath(segs: readonly string[]): string {
  return segs.map((s) => (BARE.test(s) ? s : `"${s.replace(/["\\]/g, '\\$&')}"`)).join('.');
}

export function splitPath(path: string): string[] {
  const segs: string[] = [];
  let cur = '';
  let quoted = false;
  for (let i = 0; i < path.length; i++) {
    const c = path[i]!;
    if (quoted) {
      if (c === '\\') cur += path[++i] ?? '';
      else if (c === '"') quoted = false;
      else cur += c;
    } else if (c === '"') quoted = true;
    else if (c === '.') {
      segs.push(cur);
      cur = '';
    } else cur += c;
  }
  if (path.length > 0) segs.push(cur);
  return segs;
}

export function getAt(value: JsonValue | undefined, segs: readonly string[]): JsonValue | undefined {
  let cur: JsonValue | undefined = value;
  for (const s of segs) {
    if (cur === null || cur === undefined || typeof cur !== 'object') return undefined;
    cur = Array.isArray(cur) ? cur[Number(s)] : (cur as { [k: string]: JsonValue })[s];
  }
  return cur;
}

export function isRecord(v: unknown): v is { [k: string]: JsonValue } {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

export function clone<T>(v: T): T {
  return v === undefined ? v : (JSON.parse(JSON.stringify(v)) as T);
}

export function equal(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}
