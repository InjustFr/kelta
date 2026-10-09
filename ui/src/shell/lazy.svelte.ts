// Lazily loaded components (pane views, sheets, palette). `ensure()` starts the dynamic import once;
// `lazyComponents.get()` is reactive, so templates re-render when the chunk arrives. Call `ensure`
// from an effect, never from a template expression.

import type { AnyComponent, LazyComponent } from '$app/registry';

interface Entry {
  component: AnyComponent | null;
  error: Error | null;
}

class LazyComponents {
  #entries = $state<Record<string, Entry>>({});
  #started = new Set<string>();

  get(key: string): Entry {
    return this.#entries[key] ?? { component: null, error: null };
  }

  ensure(key: string, loader: LazyComponent): void {
    if (this.#started.has(key)) return;
    this.#started.add(key);
    loader().then(
      (m) => (this.#entries[key] = { component: m.default, error: null }),
      (err: unknown) => {
        this.#started.delete(key);
        this.#entries[key] = { component: null, error: err instanceof Error ? err : new Error(String(err)) };
      },
    );
  }

  /** Forgets a failed load so `ensure` can retry. */
  reset(key: string): void {
    this.#started.delete(key);
    delete this.#entries[key];
  }
}

export const lazyComponents = new LazyComponents();
