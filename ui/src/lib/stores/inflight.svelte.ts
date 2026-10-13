// Rows whose action is running (#398): Now, Tickets and Reviews show a spinner in the row's lamp slot
// until the action settles, so a slow transition, start or merge is not mistaken for a dead click.

import { SvelteMap } from 'svelte/reactivity';

/** Running actions per row key (a count: two actions on one row keep the spinner until both end). */
const running = new SvelteMap<string, number>();

export const inflight = {
  has: (key: string): boolean => running.has(key),

  /** Runs `action` with `key`'s spinner shown; the result and errors pass through. */
  async track<T>(key: string, action: () => T | Promise<T>): Promise<T> {
    running.set(key, (running.get(key) ?? 0) + 1);
    try {
      return await action();
    } finally {
      const n = (running.get(key) ?? 1) - 1;
      if (n > 0) running.set(key, n);
      else running.delete(key);
    }
  },
};
