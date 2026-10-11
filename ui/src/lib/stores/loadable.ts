// Async data slot used by the stores: data + loading/error/stale flags (ARCH §12.2 pane states).

import type { KeltaError } from '$lib/gen';
import { toIpcError } from '$lib/ipc/transport';

export interface Loadable<T> {
  data: T | null;
  loading: boolean;
  error: KeltaError | null;
  /** Backend served cached data (provider_cache) or the last refresh failed. */
  stale: boolean;
  /** `Date.now()` of the last successful load. */
  fetchedAt: number | null;
  /** A UiEvent announced a change; the next read should refetch. */
  invalidated: boolean;
}

export function idle<T>(): Loadable<T> {
  return { data: null, loading: false, error: null, stale: false, fetchedAt: null, invalidated: false };
}

export function toKeltaError(err: unknown, command = 'ui'): KeltaError {
  return toIpcError(command, err).toKeltaError();
}

/** Detail maps only grow with the rows visited: past `max` entries the oldest fetch goes (in-flight ones stay). */
export function capDetails<T>(map: Record<string, Loadable<T>>, max = 200): Record<string, Loadable<T>> {
  const keys = Object.keys(map);
  if (keys.length <= max) return map;
  const age = (k: string): number => map[k]?.fetchedAt ?? Infinity;
  const keep = new Set(keys.sort((a, b) => age(b) - age(a)).slice(0, max));
  return Object.fromEntries(Object.entries(map).filter(([k]) => keep.has(k)));
}

/** Runs `fetch` and returns the next slot state (keeps previous data on error, marked stale). */
export async function settle<T>(
  prev: Loadable<T>,
  fetch: () => Promise<T>,
  opts: { stale?: (data: T) => boolean } = {},
): Promise<Loadable<T>> {
  try {
    const data = await fetch();
    return {
      data,
      loading: false,
      error: null,
      stale: opts.stale ? opts.stale(data) : false,
      fetchedAt: Date.now(),
      invalidated: false,
    };
  } catch (err) {
    return {
      ...prev,
      loading: false,
      error: toKeltaError(err),
      stale: prev.data !== null,
      invalidated: false,
    };
  }
}
