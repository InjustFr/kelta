// Peek & reply (ticket #139): a popover over a waiting session, opened by hovering a lamp or a Now row,
// or by `attention.peek`. It reads `session_text_tail` and writes through `session_write`: no terminal
// view is attached, so a hidden pane stays unmounted.

import type { SessionId, SessionInfo } from '$lib/gen';
import { sessionWrite } from '$lib/ipc/commands';
import { sessions } from '$lib/stores';
import { attentionRank } from '$lib/stores/reducers';

import { hud } from './hud.svelte';
import { railProjects } from './nav';

export const HOVER_MS = 350;
/** Grace to move the pointer from the anchor into the popover. */
export const LEAVE_MS = 250;
export const PEEK_WIDTH = 480;

class PeekStore {
  id = $state<SessionId | null>(null);
  /** Anchor point (below or beside the hovered lamp); null centres it at the top. */
  at = $state<{ x: number; y: number } | null>(null);
  /** The popover holds the keyboard (opened by key or entered by the pointer). */
  focused = $state(false);
  /** Focus to give back on close (the pane Louis was in). */
  previous: Element | null = null;
  #timer: ReturnType<typeof setTimeout> | undefined;

  open(id: SessionId, anchor: DOMRect | null, focus: boolean): void {
    clearTimeout(this.#timer);
    this.id = id;
    // Beside the anchor (a lamp), or below it when that runs off the window (a Now row).
    const beside = anchor !== null && anchor.right + 6 + PEEK_WIDTH <= window.innerWidth;
    this.at = !anchor
      ? null
      : beside
        ? { x: anchor.right + 6, y: anchor.top }
        : { x: Math.max(8, Math.min(anchor.left, window.innerWidth - PEEK_WIDTH - 8)), y: anchor.bottom + 4 };
    if (focus) this.#grab();
  }

  /** Pointer left the anchor or the popover: close unless it holds the keyboard. */
  leave(): void {
    clearTimeout(this.#timer);
    if (this.focused) return;
    // one-shot: hover grace
    this.#timer = setTimeout(() => this.close(false), LEAVE_MS);
  }

  /** Pointer is back on the anchor: keep the popover. */
  keep(): void {
    clearTimeout(this.#timer);
  }

  /** Pointer is over the popover: keep it, and give it the keyboard. */
  hold(): void {
    clearTimeout(this.#timer);
    this.#grab();
  }

  #grab(): void {
    if (this.focused) return;
    this.previous = typeof document === 'undefined' ? null : document.activeElement;
    this.focused = true;
  }

  close(restore = true): void {
    clearTimeout(this.#timer);
    const prev = this.previous;
    this.id = null;
    this.focused = false;
    this.previous = null;
    if (restore && prev instanceof HTMLElement && prev.isConnected) prev.focus();
  }

  /** `Tab`: the next needs-input session in any project, cycling. */
  next(): void {
    const s = sessions.next(
      railProjects().map((p) => p.id),
      this.id,
    );
    if (s) this.id = s.id;
  }
}

export const peek = new PeekStore();

/** `attention.peek`: the first session needing input, with the keyboard. */
export function peekWaiting(): void {
  if (peek.id && !peek.focused) return peek.hold();
  if (peek.id) return peek.next();
  const s = sessions.next(
    railProjects().map((p) => p.id),
    null,
  );
  if (s) peek.open(s.id, null, true);
  else hud.show('nothing waiting');
}

/** The session a lamp stands for: the most urgent one that lights it (working counts). */
export function peekTarget(list: readonly (SessionInfo | null)[]): SessionId | null {
  const lit = list.filter((s): s is SessionInfo => !!s && (s.attention !== 'none' || s.status === 'working'));
  return lit.sort((a, b) => attentionRank(b.attention) - attentionRank(a.attention))[0]?.id ?? null;
}

/** `use:peekHover={() => id}`: opens the popover after a short hover over the node. */
export function peekHover(node: HTMLElement, target: () => SessionId | null) {
  let get = target;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const enter = (): void => {
    clearTimeout(timer);
    if (peek.id && peek.id === get()) return peek.keep();
    // one-shot: hover intent
    timer = setTimeout(() => {
      const id = get();
      if (id) peek.open(id, node.getBoundingClientRect(), false);
    }, HOVER_MS);
  };
  const leave = (): void => {
    clearTimeout(timer);
    if (peek.id) peek.leave();
  };
  node.addEventListener('mouseenter', enter);
  node.addEventListener('mouseleave', leave);
  return {
    update(next: () => SessionId | null) {
      get = next;
    },
    destroy() {
      clearTimeout(timer);
      node.removeEventListener('mouseenter', enter);
      node.removeEventListener('mouseleave', leave);
    },
  };
}

/** A typed reply: bracketed paste, then Enter as its own write (like `work_send`). */
export async function sendReply(id: SessionId, text: string): Promise<void> {
  // No control bytes: an ESC could close the bracket and run keys in Claude's TUI.
  const clean = [...text].filter((c) => c >= ' ' && c !== '\x7f').join('');
  await sessionWrite(id, `\x1b[200~${clean}\x1b[201~`);
  await sessionWrite(id, '\r');
}
