// The rail's width and collapsed state (#362), kept in the webview's storage like the jumplist.

const KEY = 'kelta.rail.v1';
export const SIDEBAR_MIN = 200;
export const SIDEBAR_MAX = 480;
export const SIDEBAR_DEFAULT = 272;
/** Below this window width the rail starts as the icon column. */
const NARROW = '(max-width: 1399px)';

const clamp = (w: number): number => Math.round(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, w)));

class SidebarStore {
  width = $state(SIDEBAR_DEFAULT);
  collapsed = $state(false);
  narrow = $state(false);
  /** Opened by hand in a narrow window: stays open until the next start. */
  #opened = $state(false);
  /** The Queue is shown (#431): it lists the work the rail groups, so the rail folds to its lamps. */
  #folded = $state(false);
  /** Expanded by hand while folded: stays open until the view changes. */
  #unfolded = $state(false);

  constructor() {
    try {
      const saved = JSON.parse(localStorage.getItem(KEY) ?? 'null') as {
        width?: unknown;
        collapsed?: unknown;
      } | null;
      if (typeof saved?.width === 'number') this.width = clamp(saved.width);
      if (typeof saved?.collapsed === 'boolean') this.collapsed = saved.collapsed;
    } catch {
      // storage unavailable or corrupt: defaults
    }
    const mql = typeof matchMedia === 'function' ? matchMedia(NARROW) : null;
    this.narrow = mql?.matches ?? false;
    mql?.addEventListener('change', (e) => (this.narrow = e.matches));
  }

  get compact(): boolean {
    return this.collapsed || (this.narrow && !this.#opened) || (this.#folded && !this.#unfolded);
  }

  /** Fold while a view that repeats the rail is shown; a change of view drops a hand expansion. */
  fold(on: boolean): void {
    if (on === this.#folded) return;
    this.#folded = on;
    this.#unfolded = false;
  }

  toggle(): void {
    const open = this.compact;
    this.collapsed = !open;
    this.#opened = open && this.narrow;
    this.#unfolded = open && this.#folded;
    this.#save();
  }

  /** `persist = false` while a drag is under way: it saves once, on release. */
  resize(width: number, persist = true): void {
    this.width = clamp(width);
    if (persist) this.#save();
  }

  #save(): void {
    try {
      localStorage.setItem(KEY, JSON.stringify({ width: this.width, collapsed: this.collapsed }));
    } catch {
      // storage unavailable: the state lives for this window only
    }
  }
}

export const sidebar = new SidebarStore();
