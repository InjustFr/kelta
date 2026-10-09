// @kelta/plugin-sdk — the API of Kelta plugin screens (PLUGINS.md §7). MIT, no dependencies.
//
// A screen runs in a sandboxed iframe (`allow-scripts allow-forms`, opaque origin, no network, no
// storage). `connect()` announces itself to the parent with `{type: "kelta:ready"}`; the host
// answers with `{type: "kelta:init", …}` and transfers a MessagePort. Every call is a request
// `{id, method, params}` on that port, answered by `{id, result}` or `{id, error}`. The host also
// pushes `{type: "event" | "theme" | "params" | "visibility"}` messages.

export const KELTA_API = '0.1';

export interface KeltaErrorShape {
  code: string;
  message: string;
  detail?: unknown;
}

/** Rejection of every failed call (`code` is a Kelta `ErrorCode`, e.g. `permission_denied`). */
export class KeltaError extends Error implements KeltaErrorShape {
  readonly code: string;
  readonly detail?: unknown;

  constructor(e: KeltaErrorShape) {
    super(e.message);
    this.name = 'KeltaError';
    this.code = e.code;
    this.detail = e.detail;
  }
}

export interface ScreenInit {
  api: string;
  instance: string;
  plugin: string;
  project: string | null;
  params: unknown;
  theme: Record<string, string>;
}

export type EventCallback = (payload: unknown, name: string) => void;

export interface FetchInit {
  method?: string;
  headers?: Record<string, string>;
  body?: string;
}

export interface FetchResult {
  status: number;
  headers: Record<string, string>;
  /** Text, or base64 when `body_base64`. */
  body: string;
  body_base64: boolean;
}

export interface Kelta {
  readonly init: ScreenInit;
  /** CSS variable map (also applied to `:root`). Updated on theme changes. */
  readonly theme: Record<string, string>;
  /** Current params (updated by the host). */
  readonly params: unknown;
  call<T = unknown>(method: string, params?: unknown): Promise<T>;
  app: { info(): Promise<{ version: string; platform: string; theme: string }> };
  projects: { current(): Promise<unknown>; list(): Promise<unknown[]> };
  tickets: {
    list(params?: { scope?: 'project' | 'all'; view_id?: string; cursor?: unknown }): Promise<unknown>;
    get(ticket: unknown): Promise<unknown>;
    transitions(ticket: unknown): Promise<unknown>;
    columns(): Promise<unknown>;
    transition(ticket: unknown, transitionId: string): Promise<unknown>;
    comment(ticket: unknown, markdown: string): Promise<unknown>;
    assign(ticket: unknown, assignee: unknown): Promise<unknown>;
  };
  reviews: {
    list(params?: { scope?: 'project' | 'all'; kind?: string }): Promise<unknown>;
    get(review: unknown): Promise<unknown>;
    approve(review: unknown, headSha: string): Promise<unknown>;
    comment(review: unknown, body: string): Promise<unknown>;
    requestChanges(review: unknown, body: string): Promise<unknown>;
  };
  sessions: {
    list(params?: { project?: string }): Promise<unknown>;
    spawn(params: Record<string, unknown>): Promise<unknown>;
    sendText(sessionId: string, text: string, bracketed?: boolean): Promise<unknown>;
  };
  tools: { open(toolId: string, placement?: string): Promise<unknown> };
  /** Subscribes (needs `events:<glob>`); returns an unsubscribe function. */
  events: { on(name: string, cb: EventCallback): () => void };
  settings: {
    /** Own namespace values (+ `$effective` with `settings.read`). */
    get<T = Record<string, unknown>>(): Promise<T>;
    set(key: string, value: unknown): Promise<unknown>;
  };
  fetch(url: string, init?: FetchInit): Promise<FetchResult>;
  ui: {
    toast(text: string, level?: 'info' | 'warn' | 'error'): Promise<unknown>;
    openScreen(screenId: string, params?: unknown): Promise<unknown>;
    focus(target: { project?: string; session?: string }): Promise<unknown>;
  };
  notify(title: string, body?: string): Promise<unknown>;
  clipboard: { write(text: string): Promise<unknown> };
  onVisibility(cb: (visible: boolean) => void): () => void;
  onParams(cb: (params: unknown) => void): () => void;
  onTheme(cb: (theme: Record<string, string>) => void): () => void;
}

export interface ConnectOptions {
  /** The screen's window (tests). Defaults to `window`. */
  window?: Window;
}

interface Pending {
  resolve: (v: unknown) => void;
  reject: (e: unknown) => void;
}

type Listener<T> = (v: T) => void;

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null;
}

function applyTheme(win: Window, tokens: Record<string, string>): void {
  const root = win.document?.documentElement;
  if (!root) return;
  for (const [k, v] of Object.entries(tokens)) {
    if (k.startsWith('--')) root.style.setProperty(k, v);
    else if (k === 'color-scheme') root.style.colorScheme = v;
  }
}

function sub<T>(set: Set<Listener<T>>, cb: Listener<T>): () => void {
  set.add(cb);
  return () => {
    set.delete(cb);
  };
}

function createClient(win: Window, init: ScreenInit, port: MessagePort): Kelta {
  let nextId = 1;
  let theme = { ...init.theme };
  let params = init.params;
  const pending = new Map<number, Pending>();
  const events = new Map<string, Set<EventCallback>>();
  const visibility = new Set<Listener<boolean>>();
  const paramsListeners = new Set<Listener<unknown>>();
  const themeListeners = new Set<Listener<Record<string, string>>>();

  const call = <T = unknown>(method: string, p: unknown = {}): Promise<T> =>
    new Promise<T>((resolve, reject) => {
      const id = nextId++;
      pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
      port.postMessage({ id, method, params: p ?? {} });
    });

  port.onmessage = (ev: MessageEvent) => {
    const m: unknown = ev.data;
    if (!isObject(m)) return;
    if (typeof m.id === 'number' && pending.has(m.id)) {
      const p = pending.get(m.id)!;
      pending.delete(m.id);
      if (isObject(m.error)) p.reject(new KeltaError(m.error as unknown as KeltaErrorShape));
      else p.resolve(m.result);
      return;
    }
    switch (m.type) {
      case 'event':
        for (const [glob, cbs] of events) {
          if (matches(glob, String(m.name))) for (const cb of [...cbs]) cb(m.payload, String(m.name));
        }
        break;
      case 'theme':
        if (isObject(m.tokens)) {
          theme = { ...(m.tokens as Record<string, string>) };
          applyTheme(win, theme);
          for (const cb of [...themeListeners]) cb(theme);
        }
        break;
      case 'params':
        params = m.params;
        for (const cb of [...paramsListeners]) cb(params);
        break;
      case 'visibility':
        for (const cb of [...visibility]) cb(Boolean(m.visible));
        break;
    }
  };
  port.start?.();
  applyTheme(win, theme);

  return {
    init,
    get theme() {
      return theme;
    },
    get params() {
      return params;
    },
    call,
    app: { info: () => call('app.info') },
    projects: { current: () => call('projects.current'), list: () => call('projects.list') },
    tickets: {
      list: (p = {}) => call('tickets.list', p),
      get: (ticket) => call('tickets.get', { ticket }),
      transitions: (ticket) => call('tickets.transitions', { ticket }),
      columns: () => call('tickets.columns'),
      transition: (ticket, transition_id) => call('tickets.transition', { ticket, transition_id }),
      comment: (ticket, markdown) => call('tickets.comment', { ticket, markdown }),
      assign: (ticket, assignee) => call('tickets.assign', { ticket, assignee }),
    },
    reviews: {
      list: (p = {}) => call('reviews.list', p),
      get: (review) => call('reviews.get', { review }),
      approve: (review, head_sha) => call('reviews.approve', { review, head_sha }),
      comment: (review, body) => call('reviews.comment', { review, body }),
      requestChanges: (review, body) => call('reviews.request_changes', { review, body }),
    },
    sessions: {
      list: (p = {}) => call('sessions.list', p),
      spawn: (p) => call('sessions.spawn', p),
      sendText: (session_id, text, bracketed) => call('sessions.send_text', { session_id, text, bracketed }),
    },
    tools: { open: (tool_id, placement) => call('tools.open', { tool_id, placement }) },
    events: {
      on(name, cb) {
        let set = events.get(name);
        if (!set) {
          set = new Set();
          events.set(name, set);
          call('events.subscribe', { names: [name] }).catch((e: unknown) => {
            events.delete(name);
            console.warn(`[kelta] events.subscribe(${name}) failed`, e);
          });
        }
        set.add(cb);
        return () => {
          const s = events.get(name);
          if (!s) return;
          s.delete(cb);
          if (s.size === 0) {
            events.delete(name);
            void call('events.unsubscribe', { names: [name] }).catch(() => {});
          }
        };
      },
    },
    settings: {
      get: () => call('settings.get'),
      set: (key, value) => call('settings.set', { key, value }),
    },
    fetch: (url, i = {}) => call('net.fetch', { url, ...i }),
    ui: {
      toast: (text, level) => call('ui.toast', { text, level }),
      openScreen: (screen_id, p) => call('ui.open_screen', { screen_id, params: p ?? null }),
      focus: (target) => call('ui.focus', target),
    },
    notify: (title, body) => call('notify.send', { title, body }),
    clipboard: { write: (text) => call('clipboard.write', { text }) },
    onVisibility: (cb) => sub(visibility, cb),
    onParams: (cb) => sub(paramsListeners, cb),
    onTheme: (cb) => sub(themeListeners, cb),
  };
}

/** Glob match where `*` matches any run of characters and `?` one character. */
export function matches(glob: string, name: string): boolean {
  if (glob === name) return true;
  const re = new RegExp(
    `^${glob
      .split('')
      .map((c) => (c === '*' ? '.*' : c === '?' ? '.' : c.replace(/[.+^${}()|[\]\\]/g, '\\$&')))
      .join('')}$`,
  );
  return re.test(name);
}

let connecting: Promise<Kelta> | null = null;

/**
 * Waits for the host's `kelta:init` (only from the parent window) and returns the connected API.
 * Idempotent: later calls return the same connection.
 */
export function connect(options: ConnectOptions = {}): Promise<Kelta> {
  if (connecting && !options.window) return connecting;
  const win = options.window ?? window;
  const p = new Promise<Kelta>((resolve) => {
    const onMessage = (ev: MessageEvent): void => {
      if (ev.source !== win.parent) return;
      const m: unknown = ev.data;
      if (!isObject(m) || m.type !== 'kelta:init') return;
      const port = ev.ports[0];
      if (!port) return;
      win.removeEventListener('message', onMessage);
      const init: ScreenInit = {
        api: String(m.api ?? KELTA_API),
        instance: String(m.instance ?? ''),
        plugin: String(m.plugin ?? ''),
        project: typeof m.project === 'string' ? m.project : null,
        params: m.params ?? null,
        theme: isObject(m.theme) ? (m.theme as Record<string, string>) : {},
      };
      resolve(createClient(win, init, port));
    };
    win.addEventListener('message', onMessage);
    win.parent.postMessage({ type: 'kelta:ready', api: KELTA_API }, '*');
  });
  if (!options.window) connecting = p;
  return p;
}
