// xterm view pool (ARCHITECTURE §9.1). One `TerminalView` (an xterm instance bound to one session)
// per session id. Capacity = `terminal.max_live_views` hidden views, plus every visible pane.
// Hidden views beyond the capacity are disposed (and detached from their session) in LRU order.
//
// This module is xterm-free: the xterm-backed view lives in `./view` and is imported on first use,
// so the pool logic (and its tests) never loads the terminal library.

import type { SessionId } from '$lib/gen';

import type { TerminalConfig } from './config';
import type { TerminalViewDeps } from './view';

export { DEFAULT_CONFIG, configFromSettings, type TerminalConfig } from './config';
export { decodeFrame, AckBatcher, FrameHandler } from './frames';
export { frames, FrameScheduler } from './raf';
export type { TerminalViewDeps, TerminalView, ViewState, ViewRequest } from './view';

export interface TerminalViewPoolOptions {
  /** `terminal.max_live_views` (default 2, 1..12): hidden views kept alive beyond the visible ones. */
  capacity: number;
  /** View factory (tests inject fakes). The default creates xterm-backed views lazily. */
  createView?: (sessionId: SessionId) => PoolView | Promise<PoolView>;
}

/** What the pool needs from a view. */
export interface PoolView {
  readonly id: SessionId;
  /** Puts the view's DOM into `container` (re-parenting if it was shown elsewhere). */
  mount(container: HTMLElement): void;
  /** Takes the DOM out of the document; the view stays attached to its session. */
  unmount(): void;
  /** Attaches to the session (idempotent). Resolves when the attach completed. */
  attach(): Promise<void>;
  focus(): void;
  /** Applies new settings / theme to a live view. */
  applyConfig?(config: TerminalConfig): void;
  /** Frees the xterm instance and detaches from the session. */
  dispose(): void;
}

export const MIN_CAPACITY = 1;
export const MAX_CAPACITY = 12;

export function clampCapacity(n: number): number {
  if (!Number.isFinite(n)) return 4;
  return Math.min(MAX_CAPACITY, Math.max(MIN_CAPACITY, Math.round(n)));
}

export class TerminalViewPool {
  capacity: number;

  #createView: (sessionId: SessionId) => PoolView | Promise<PoolView>;
  #views = new Map<SessionId, PoolView>();
  #creating = new Map<SessionId, Promise<PoolView>>();
  #visible = new Set<SessionId>();
  /** Hidden views, least recently hidden first. */
  #hidden: SessionId[] = [];
  #protected = new Set<SessionId>();
  #evictQueued = false;
  #deps: TerminalViewDeps | null = null;
  /** Called when a view was evicted (perf HUD, tests). */
  onEvict: (sessionId: SessionId) => void = () => {};

  constructor(options: TerminalViewPoolOptions) {
    this.capacity = clampCapacity(options.capacity);
    this.#createView = options.createView ?? ((id) => this.#createXtermView(id));
  }

  /** Shared dependencies of the default (xterm) views: config, key routing, platform. */
  configure(deps: TerminalViewDeps): void {
    this.#deps = deps;
  }

  async #createXtermView(sessionId: SessionId): Promise<PoolView> {
    const deps = this.#deps;
    if (!deps) throw new Error('TerminalViewPool.configure() was not called');
    const { TerminalView } = await import('./view');
    return new TerminalView(sessionId, deps);
  }

  /** Shows the session's view inside `container` (attaching or re-using a pooled view). */
  async show(sessionId: SessionId, container: HTMLElement): Promise<void> {
    this.#visible.add(sessionId);
    this.#protected.delete(sessionId);
    this.#hidden = this.#hidden.filter((id) => id !== sessionId);

    let view = this.#views.get(sessionId);
    if (!view) {
      let pending = this.#creating.get(sessionId);
      if (!pending) {
        pending = Promise.resolve(this.#createView(sessionId));
        this.#creating.set(sessionId, pending);
      }
      try {
        view = await pending;
      } finally {
        this.#creating.delete(sessionId);
      }
      const existing = this.#views.get(sessionId);
      if (existing && existing !== view) {
        view.dispose();
        view = existing;
      } else {
        this.#views.set(sessionId, view);
      }
      if (!this.#visible.has(sessionId)) {
        // Hidden again while being created: hide() pooled the id. Released or evicted meanwhile: drop it.
        if (!this.#hidden.includes(sessionId)) {
          this.#views.delete(sessionId);
          view.dispose();
        }
        return;
      }
    }
    view.mount(container);
    await view.attach();
  }

  /** The pane stops showing the view; it stays pooled (LRU) until evicted. */
  hide(sessionId: SessionId): void {
    if (!this.#visible.delete(sessionId)) return;
    const view = this.#views.get(sessionId);
    view?.unmount(); // no view yet: still being created, show() keeps it only if still pooled
    this.#pushHidden(sessionId);
  }

  /** Disposes the view and detaches the session. */
  release(sessionId: SessionId): void {
    this.#visible.delete(sessionId);
    this.#protected.delete(sessionId);
    this.#hidden = this.#hidden.filter((id) => id !== sessionId);
    const view = this.#views.get(sessionId);
    this.#views.delete(sessionId);
    view?.dispose();
  }

  focus(sessionId: SessionId): void {
    this.#views.get(sessionId)?.focus();
  }

  /** Applies settings / theme changes to every live view. */
  applyConfig(config: TerminalConfig): void {
    for (const view of this.#views.values()) view.applyConfig?.(config);
  }

  setCapacity(capacity: number): void {
    this.capacity = clampCapacity(capacity);
    this.#queueEvict();
  }

  /**
   * Views about to be shown (a project switch): they are skipped by eviction until `show` runs, so
   * hiding the previous project's views cannot push them out first.
   */
  protect(sessionIds: Iterable<SessionId>): void {
    this.#protected = new Set(sessionIds);
  }

  /** Marks hidden views as recently used (keeps them for longer). */
  touch(sessionIds: Iterable<SessionId>): void {
    const touched = [...sessionIds].filter((id) => this.#hidden.includes(id));
    if (touched.length === 0) return;
    this.#hidden = [...this.#hidden.filter((id) => !touched.includes(id)), ...touched];
  }

  get<V extends PoolView = PoolView>(sessionId: SessionId): V | null {
    return (this.#views.get(sessionId) as V | undefined) ?? null;
  }

  has(sessionId: SessionId): boolean {
    return this.#views.has(sessionId);
  }

  /** Number of live xterm instances. */
  get liveCount(): number {
    return this.#views.size;
  }

  get visibleCount(): number {
    return this.#visible.size;
  }

  /** Hidden pooled views, least recently hidden first (eviction order). */
  get hiddenOrder(): readonly SessionId[] {
    return this.#hidden;
  }

  /** Evicts now instead of at the end of the current task (tests, settings change). */
  evictNow(): void {
    this.#evictQueued = false;
    while (this.#hidden.length > this.capacity) {
      const victim = this.#hidden.find((id) => !this.#protected.has(id));
      if (victim === undefined) break;
      this.#hidden = this.#hidden.filter((id) => id !== victim);
      const view = this.#views.get(victim);
      this.#views.delete(victim);
      view?.dispose();
      this.onEvict(victim);
    }
  }

  #pushHidden(sessionId: SessionId): void {
    this.#hidden = [...this.#hidden.filter((id) => id !== sessionId), sessionId];
    this.#queueEvict();
  }

  /** Eviction runs after the current flush so a project switch can show its views first. */
  #queueEvict(): void {
    if (this.#evictQueued) return;
    this.#evictQueued = true;
    queueMicrotask(() => {
      if (this.#evictQueued) this.evictNow();
    });
  }
}

/** The window-wide pool (configured by the shell). */
export const terminalPool = new TerminalViewPool({ capacity: 4 });
