// Where each ticket stands in its pipeline (#461, #462): `ticket_steps` at startup, then `ticket.step`.
import type { TicketRef, TicketStep, UiEvent } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';

import { ticketKey } from './tickets.svelte';

export class TicketStepsStore {
  byKey = $state<Record<string, TicketStep>>({});

  get(ref: TicketRef): TicketStep | null {
    return this.byKey[ticketKey(ref)] ?? null;
  }

  apply(ev: UiEvent): void {
    if (ev.type === 'ticket.step') this.byKey = { ...this.byKey, [ticketKey(ev.step.ticket)]: ev.step };
  }

  async load(): Promise<void> {
    const rows = await ipc.ticketSteps();
    // An event that landed during the load is newer than its row.
    this.byKey = { ...Object.fromEntries(rows.map((r) => [ticketKey(r.ticket), r])), ...this.byKey };
  }
}
