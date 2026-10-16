// Toast queue (ARCH §12.2). Fed by `toast` UiEvents and by UI code (`toasts.error(err)`).
// Auto-dismiss uses one one-shot timer per toast, armed when the toast is pushed.

import { t } from '$lib/i18n/index.svelte';
import { dispatch } from '$lib/actions';
import type { Toast, UiEvent } from '$lib/gen';
import { toIpcError } from '$lib/ipc/transport';

export interface ToastEntry {
  id: number;
  toast: Toast;
  createdAt: number;
  /** The view it was raised in (`viewKey()`); `null` = background work, tied to no view. */
  view: string | null;
}

type PushOpts = { timeoutMs?: number; view?: string | null };

const DEFAULT_TIMEOUT_MS: Record<Toast['level'], number> = { info: 5000, warn: 8000, error: 12000 };
const MAX_TOASTS = 5;

export class ToastsStore {
  list = $state<ToastEntry[]>([]);
  #nextId = 1;
  /** Focus mode (#421): the background info toasts held for its summary; `null` = not focused. */
  held = $state<string[] | null>(null);
  /** The desktop notifications the backend held meanwhile (`focus.held`, #442). */
  heldRemote = $state(0);
  /** The view on screen, stamped on each toast (wired by ToastHost: the stores know no layout). */
  where: () => string | null = () => null;
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- non-reactive bookkeeping
  #timers = new Map<number, ReturnType<typeof setTimeout>>();

  /**
   * Shows a toast. `timeoutMs: 0` keeps it until dismissed. `view` overrides the view it is tied to
   * (`null`: none, for background work). Returns its id (0 when focus mode held it).
   */
  push(toast: Toast, opts: PushOpts = {}): number {
    const view = opts.view === undefined ? this.where() : opts.view;
    // Errors, warnings, toasts with an action and those of the view on screen still show.
    if (this.held && view === null && toast.level === 'info' && !toast.action) {
      this.held.push(toast.text);
      return 0;
    }
    const id = this.#nextId++;
    this.list = [...this.list, { id, toast, createdAt: Date.now(), view }].slice(-MAX_TOASTS);
    const timeout = opts.timeoutMs ?? DEFAULT_TIMEOUT_MS[toast.level];
    if (timeout > 0) {
      // one-shot: auto-dismiss this toast
      this.#timers.set(
        id,
        setTimeout(() => this.dismiss(id), timeout),
      );
    }
    return id;
  }

  info(text: string, opts?: PushOpts): number {
    return this.push({ level: 'info', text, action: null }, opts);
  }

  warn(text: string): number {
    return this.push({ level: 'warn', text, action: null });
  }

  /** Toast for a failed operation (any thrown value; IPC errors keep their message). A core error naming a setting (`detail.setting`, #477) offers to open it. */
  error(err: unknown, context?: string): number {
    const e = typeof err === 'string' ? null : toIpcError(context ?? 'ui', err);
    const message = e ? e.message : (err as string);
    const key = (e?.detail as { setting?: unknown } | null | undefined)?.setting;
    return this.push({
      level: 'error',
      text: context ? `${context}: ${message}` : message,
      action:
        typeof key === 'string'
          ? { label: t('Open setting'), command: 'settings.open', args: { key } }
          : null,
    });
  }

  /** The most recent toast still shown that has an action (`toast.run_last`, ⇧⌘O). */
  get lastActionable(): ToastEntry | null {
    return this.list.findLast((t) => t.toast.action) ?? null;
  }

  /** Dismisses a toast and runs its action (or its `secondary` one). */
  async run(id: number, secondary = false): Promise<void> {
    const toast = this.list.find((t) => t.id === id)?.toast;
    const action = secondary ? toast?.secondary : toast?.action;
    this.dismiss(id);
    if (!action) return;
    const args =
      action.args && typeof action.args === 'object' && !Array.isArray(action.args)
        ? (action.args as Record<string, unknown>)
        : undefined;
    try {
      if (!(await dispatch(action.command, args)))
        this.warn(t('No handler for “{action}”', { action: action.label }));
    } catch (err) {
      this.error(err, action.label);
    }
  }

  dismiss(id: number): void {
    const timer = this.#timers.get(id);
    if (timer !== undefined) clearTimeout(timer);
    this.#timers.delete(id);
    this.list = this.list.filter((t) => t.id !== id);
  }

  /** `view` is now on screen: closes the info toasts raised elsewhere. Errors, warnings and toasts with an action stay. */
  leave(view: string): void {
    for (const e of this.list)
      if (e.view !== null && e.view !== view && e.toast.level === 'info' && !e.toast.action)
        this.dismiss(e.id);
  }

  clear(): void {
    for (const timer of this.#timers.values()) clearTimeout(timer);
    this.#timers.clear();
    this.list = [];
  }

  apply(ev: UiEvent): void {
    // A toast asking a choice stays until answered.
    if (ev.type === 'toast')
      this.push(ev.toast, { view: null, ...(ev.toast.action && ev.toast.secondary && { timeoutMs: 0 }) });
    else if (ev.type === 'focus.held') this.heldRemote = ev.count;
  }
}
