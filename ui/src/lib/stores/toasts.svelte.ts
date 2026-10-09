// Toast queue (ARCH §12.2). Fed by `toast` UiEvents and by UI code (`toasts.error(err)`).
// Auto-dismiss uses one one-shot timer per toast, armed when the toast is pushed.

import type { Toast, UiEvent } from '$lib/gen';
import { toIpcError } from '$lib/ipc/transport';

export interface ToastEntry {
  id: number;
  toast: Toast;
  createdAt: number;
}

const DEFAULT_TIMEOUT_MS: Record<Toast['level'], number> = { info: 5000, warn: 8000, error: 12000 };
const MAX_TOASTS = 5;

export class ToastsStore {
  list = $state<ToastEntry[]>([]);
  #nextId = 1;
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- non-reactive bookkeeping
  #timers = new Map<number, ReturnType<typeof setTimeout>>();

  /** Shows a toast. `timeoutMs: 0` keeps it until dismissed. Returns its id. */
  push(toast: Toast, opts: { timeoutMs?: number } = {}): number {
    const id = this.#nextId++;
    this.list = [...this.list, { id, toast, createdAt: Date.now() }].slice(-MAX_TOASTS);
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

  info(text: string): number {
    return this.push({ level: 'info', text, action: null });
  }

  warn(text: string): number {
    return this.push({ level: 'warn', text, action: null });
  }

  /** Toast for a failed operation (any thrown value; IPC errors keep their message). */
  error(err: unknown, context?: string): number {
    const message = typeof err === 'string' ? err : toIpcError(context ?? 'ui', err).message;
    return this.push({ level: 'error', text: context ? `${context}: ${message}` : message, action: null });
  }

  dismiss(id: number): void {
    const timer = this.#timers.get(id);
    if (timer !== undefined) clearTimeout(timer);
    this.#timers.delete(id);
    this.list = this.list.filter((t) => t.id !== id);
  }

  clear(): void {
    for (const timer of this.#timers.values()) clearTimeout(timer);
    this.#timers.clear();
    this.list = [];
  }

  apply(ev: UiEvent): void {
    if (ev.type === 'toast') this.push(ev.toast);
  }
}
