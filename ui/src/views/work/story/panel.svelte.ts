// The Story panel's width and per-tab open state (#423), kept in the webview's storage like the rail.

const KEY = 'kelta.story.v1';
export const STORY_MIN = 260;
export const STORY_MAX = 640;
export const STORY_DEFAULT = 340;
/** From this window width a tab without a saved state starts with the panel open. */
const WIDE = '(min-width: 1280px)';

const clamp = (w: number): number => Math.round(Math.min(STORY_MAX, Math.max(STORY_MIN, w)));

class StoryStore {
  width = $state(STORY_DEFAULT);
  wide = $state(true);
  /** shortcut: closed tabs' entries stay in storage (a few bytes each); prune them if it ever grows. */
  #open = $state<Record<string, boolean>>({});
  /** Shown for the return brief without changing the saved state. */
  #revealed = $state<string | null>(null);

  constructor() {
    try {
      const saved = JSON.parse(localStorage.getItem(KEY) ?? 'null') as {
        width?: unknown;
        open?: unknown;
      } | null;
      if (typeof saved?.width === 'number') this.width = clamp(saved.width);
      if (saved?.open && typeof saved.open === 'object') this.#open = saved.open as Record<string, boolean>;
    } catch {
      // storage unavailable or corrupt: defaults
    }
    const mql = typeof matchMedia === 'function' ? matchMedia(WIDE) : null;
    this.wide = mql?.matches ?? true;
    mql?.addEventListener('change', (e) => (this.wide = e.matches));
  }

  isOpen(tabId: string): boolean {
    return this.#revealed === tabId || (this.#open[tabId] ?? this.wide);
  }

  setOpen(tabId: string, open: boolean): void {
    this.#revealed = null;
    this.#open = { ...this.#open, [tabId]: open };
    this.#save();
  }

  toggle(tabId: string): void {
    this.setOpen(tabId, !this.isOpen(tabId));
  }

  reveal(tabId: string): void {
    this.#revealed = tabId;
  }

  /** `persist = false` while a drag is under way: it saves once, on release. */
  resize(width: number, persist = true): void {
    this.width = clamp(width);
    if (persist) this.#save();
  }

  #save(): void {
    try {
      localStorage.setItem(KEY, JSON.stringify({ width: this.width, open: this.#open }));
    } catch {
      // storage unavailable: the state lives for this window only
    }
  }
}

export const story = new StoryStore();
