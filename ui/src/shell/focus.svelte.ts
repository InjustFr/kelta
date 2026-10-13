// Focus mode (#421, `view.focus`): the rail and the docked Story panel hide, the work bar stays. While
// it is on, background info toasts and the backend's desktop notifications about other work are held
// (a blocked Claude still notifies and still lights the "needs you" chip); leaving sums them up in one toast.

import { registerAction } from '$lib/actions';
import { t } from '$lib/i18n/index.svelte';
import { notifyFocus } from '$lib/ipc/commands';
import { toasts } from '$lib/stores';

/** Items the summary names before "+N more". */
const SHOWN = 4;

/** "While you were focused: 2× PR approved, SHOP-12 ready to review", or `null` when nothing was held. */
export function heldSummary(held: readonly string[]): string | null {
  if (held.length === 0) return null;
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- local tally, not state
  const counts = new Map<string, number>();
  for (const text of held) counts.set(text, (counts.get(text) ?? 0) + 1);
  const parts = [...counts].map(([text, n]) => (n > 1 ? `${n}× ${text}` : text));
  const more = parts.length - SHOWN;
  const what = parts.slice(0, SHOWN).join(', ') + (more > 0 ? ` +${t('{n} more', { n: more })}` : '');
  return t('While you were focused: {what}', { what });
}

class FocusStore {
  on = $state(false);
  #unregister: (() => void) | null = null;
  /** The last `notify_focus` call: the next waits for it, so a quick on-off lands on the backend in order. */
  #sent: Promise<string[]> = Promise.resolve([]);

  #send(on: boolean): Promise<string[]> {
    this.#sent = this.#sent.then(
      () => notifyFocus({ on }),
      () => notifyFocus({ on }),
    );
    return this.#sent;
  }

  async toggle(): Promise<void> {
    if (this.on) return this.leave();
    this.on = true;
    toasts.held = [];
    // `Esc` `Esc` leaves (KeyManager): the action exists only while focus mode is on.
    this.#unregister = registerAction('view.unfocus', () => this.leave());
    await this.#send(true).catch((err: unknown) => toasts.error(err, t('Focus mode')));
  }

  async leave(): Promise<void> {
    if (!this.on) return;
    this.on = false;
    this.#unregister?.();
    this.#unregister = null;
    const local = toasts.held ?? [];
    toasts.held = null;
    const remote = await this.#send(false).catch(() => [] as string[]);
    const text = heldSummary([...remote, ...local]);
    if (text) toasts.info(text, { view: null });
  }
}

export const focus = new FocusStore();
