// Batch start (#141): Space marks ticket rows in Tickets, Board and Now; Mod+Enter opens the batch
// sheet. Also the Claude slot count the status bar and the sheet show.

import type { ProjectId, TicketRef } from '$lib/gen';
import { sessions, settings, ui, work } from '$lib/stores';
import { ticketKey } from '$lib/stores/tickets.svelte';

export interface Marked {
  ref: TicketRef;
  projectId: ProjectId | null;
}

class Batch {
  marked = $state<Record<string, Marked>>({});

  has(ref: TicketRef): boolean {
    return ticketKey(ref) in this.marked;
  }

  toggle(ref: TicketRef, projectId: ProjectId | null): void {
    const key = ticketKey(ref);
    const { [key]: had, ...rest } = this.marked;
    this.marked = had ? rest : { ...rest, [key]: { ref, projectId } };
  }

  clear(): void {
    this.marked = {};
  }

  get list(): Marked[] {
    return Object.values(this.marked);
  }
}

export const batch = new Batch();

/** Space marks the row's ticket, Mod+Enter opens the batch sheet; true when the key was handled. */
export function batchKey(
  e: KeyboardEvent,
  ticket: { ref: TicketRef; projectId: ProjectId | null } | null,
): boolean {
  if (e.key === 'Enter' && (e.metaKey || e.ctrlKey) && !e.altKey) {
    if (batch.list.length > 0) ui.openSheet('start_batch');
    return true;
  }
  if (e.key !== ' ' || e.metaKey || e.ctrlKey || e.altKey || !ticket) return false;
  batch.toggle(ticket.ref, ticket.projectId);
  return true;
}

/** Live Claude processes (Dormant ones do not count), the cap (0 = none) and queued items. */
export function claudeSlots(): { live: number; max: number; queued: number } {
  return {
    live: sessions.all.filter((s) => s.kind.type === 'claude' && s.lifecycle === 'live').length,
    max: settings.value()?.claude.max_live ?? 4,
    queued: work.all.filter((w) => w.state.kind === 'queued').length,
  };
}

/** How many of `n` new starts get a slot now; the rest queue. */
export function startsNow(n: number, live: number, max: number): number {
  return max === 0 ? n : Math.max(0, Math.min(n, max - live));
}
