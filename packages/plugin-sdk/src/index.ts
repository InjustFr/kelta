// @kelta/plugin-sdk — SCAFFOLD STUB (owner: L8). API surface per PLUGINS.md §7.
// L8 implements the MessageChannel handshake and the typed helpers; signatures below are the
// intended public API and may be extended (not narrowed).

export const KELTA_API = '0.1';

export interface KeltaErrorShape {
  code: string;
  message: string;
  detail?: unknown;
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

export interface Kelta {
  readonly init: ScreenInit;
  /** CSS variable map (also applied to `:root`). */
  readonly theme: Record<string, string>;
  call<T = unknown>(method: string, params?: unknown): Promise<T>;
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
  events: { on(name: string, cb: EventCallback): () => void };
  settings: { get(): Promise<unknown>; set(key: string, value: unknown): Promise<unknown> };
  fetch(
    url: string,
    init?: { method?: string; headers?: Record<string, string>; body?: string },
  ): Promise<unknown>;
  ui: {
    toast(text: string, level?: 'info' | 'warn' | 'error'): Promise<unknown>;
    openScreen(screenId: string, params?: unknown): Promise<unknown>;
    focus(target: unknown): Promise<unknown>;
  };
  notify(title: string, body: string): Promise<unknown>;
  onVisibility(cb: (visible: boolean) => void): () => void;
}

/** Waits for the host's `kelta:init` message and returns the connected API. */
export function connect(): Promise<Kelta> {
  return Promise.reject(new Error('not implemented: connect'));
}
