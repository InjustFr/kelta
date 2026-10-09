// Confirmation dialogs (destructive actions only, ARCHITECTURE §12.2). `confirms.ask()` resolves
// with the id of the chosen action, or null when dismissed. The shell renders the current request.

export interface ConfirmAction {
  id: string;
  label: string;
  variant?: 'primary' | 'secondary' | 'danger';
}

export interface ConfirmRequest {
  title: string;
  body: string;
  /** Extra lines (file names, session names…). */
  details?: string[];
  actions: ConfirmAction[];
  tone?: 'default' | 'danger';
}

interface Pending extends ConfirmRequest {
  resolve: (id: string | null) => void;
}

export class ConfirmStore {
  current = $state<Pending | null>(null);
  #queue: Pending[] = [];

  ask(request: ConfirmRequest): Promise<string | null> {
    return new Promise((resolve) => {
      const pending: Pending = { ...request, resolve };
      if (this.current) this.#queue.push(pending);
      else this.current = pending;
    });
  }

  answer(id: string | null): void {
    const cur = this.current;
    if (!cur) return;
    this.current = this.#queue.shift() ?? null;
    cur.resolve(id);
  }
}

export const confirms = new ConfirmStore();

export interface PromptRequest {
  title: string;
  label: string;
  value?: string;
  confirmLabel?: string;
}

interface PendingPrompt extends PromptRequest {
  resolve: (value: string | null) => void;
}

/** Single-line text prompt (rename). WebKit blocks `window.prompt`, so the shell renders its own. */
export class PromptStore {
  current = $state<PendingPrompt | null>(null);

  ask(request: PromptRequest): Promise<string | null> {
    return new Promise((resolve) => {
      this.current?.resolve(null);
      this.current = { ...request, resolve };
    });
  }

  answer(value: string | null): void {
    const cur = this.current;
    if (!cur) return;
    this.current = null;
    cur.resolve(value);
  }
}

export const prompts = new PromptStore();
