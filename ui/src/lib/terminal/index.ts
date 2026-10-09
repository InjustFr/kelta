// SCAFFOLD STUB (L2): xterm view pool (ARCHITECTURE §9.1). L2 replaces the implementation and
// may extend the API; the names below are what other lanes may import.

import type { SessionId } from '$lib/gen';

export interface TerminalViewPoolOptions {
  /** `terminal.max_live_views` (default 4, 1..12): hidden views kept alive beyond the visible ones. */
  capacity: number;
}

export class TerminalViewPool {
  capacity: number;

  constructor(options: TerminalViewPoolOptions) {
    this.capacity = options.capacity;
  }

  /** Shows the session's view inside `container` (attaching or re-using a pooled view). */
  async show(sessionId: SessionId, container: HTMLElement): Promise<void> {
    void sessionId;
    void container;
    throw new Error('not implemented: TerminalViewPool.show');
  }

  /** The pane stops showing the view; it stays pooled (LRU) until evicted. */
  hide(sessionId: SessionId): void {
    void sessionId;
  }

  /** Disposes the view and detaches the session. */
  release(sessionId: SessionId): void {
    void sessionId;
  }

  focus(sessionId: SessionId): void {
    void sessionId;
  }

  setCapacity(capacity: number): void {
    this.capacity = capacity;
  }

  /** Number of live xterm instances. */
  get liveCount(): number {
    return 0;
  }
}
