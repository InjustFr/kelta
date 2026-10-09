// Ticket moves (SPEC §3.4): optimistic board move with rollback, ambiguous-transition picker,
// NeedsFields form and the "no transition" toast.

import type { Column, JsonValue, Ticket, Transition } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';
import { toIpcError } from '$lib/ipc/transport';
import { tickets, toasts } from '$lib/stores';

import { parseCandidates, parseFields, type FieldSpec } from '../work/common';

export type MoveDialog =
  | { kind: 'pick'; ticket: Ticket; candidates: Transition[]; target: string }
  | {
      kind: 'fields';
      ticket: Ticket;
      transitionId: string;
      target: string;
      fields: FieldSpec[];
      message: string;
    };

export class MoveController {
  dialog = $state<MoveDialog | null>(null);
  busy = $state(false);

  cancel(): void {
    this.dialog = null;
  }

  /** Board/list move to a column: optimistic, rolled back by the store on any error. */
  async moveToColumn(ticket: Ticket, column: Column): Promise<boolean> {
    if (this.busy) return false;
    this.busy = true;
    try {
      await tickets.move(ticket, column);
      return true;
    } catch (err) {
      await this.#handle(err, ticket, column.name, column.category, null);
      return false;
    } finally {
      this.busy = false;
    }
  }

  /** "Move to…" from the detail: applies one transition (shows the form when fields are needed). */
  async moveViaTransition(ticket: Ticket, transition: Transition, fields?: JsonValue): Promise<boolean> {
    if (this.busy) return false;
    this.busy = true;
    try {
      const updated = await ipc.trackerTransition({
        ticket: ticket.ref,
        transition_id: transition.id,
        fields: fields ?? null,
      });
      tickets.patch(updated);
      this.dialog = null;
      return true;
    } catch (err) {
      await this.#handle(err, ticket, transition.to.name, transition.to.category, transition);
      return false;
    } finally {
      this.busy = false;
    }
  }

  async choose(transition: Transition): Promise<void> {
    const d = this.dialog;
    if (d?.kind !== 'pick') return;
    this.dialog = null;
    await this.moveViaTransition(d.ticket, transition);
  }

  async submitFields(values: Record<string, string>): Promise<void> {
    const d = this.dialog;
    if (d?.kind !== 'fields') return;
    if (this.busy) return;
    this.busy = true;
    try {
      const updated = await ipc.trackerTransition({
        ticket: d.ticket.ref,
        transition_id: d.transitionId,
        fields: values,
      });
      tickets.patch(updated);
      this.dialog = null;
    } catch (err) {
      const e = toIpcError('tracker_transition', err);
      if (e.code === 'needs_fields') {
        const fields = parseFields(e.detail);
        this.dialog = { ...d, fields: fields.length > 0 ? fields : d.fields, message: e.message };
      } else {
        this.dialog = null;
        toasts.error(err, `Moving ${d.ticket.ref.key}`);
      }
    } finally {
      this.busy = false;
    }
  }

  async #handle(
    err: unknown,
    ticket: Ticket,
    targetName: string,
    targetCategory: Column['category'],
    via: Transition | null,
  ): Promise<void> {
    const e = toIpcError('tracker_move', err);
    if (e.code === 'conflict') {
      const candidates = parseCandidates(e.detail);
      if (candidates.length > 0) {
        this.dialog = { kind: 'pick', ticket, candidates, target: targetName };
        return;
      }
    }
    if (e.code === 'needs_fields') {
      let transitionId = via?.id ?? null;
      if (!transitionId) {
        const detail =
          e.detail !== null && typeof e.detail === 'object' && !Array.isArray(e.detail) ? e.detail : {};
        const fromDetail = (detail as Record<string, JsonValue>).transition_id;
        transitionId = typeof fromDetail === 'string' ? fromDetail : null;
      }
      if (!transitionId) {
        const list = (await tickets.loadTransitions(ticket.ref)).data ?? [];
        const match =
          list.find((t) => t.to.name === targetName) ?? list.find((t) => t.to.category === targetCategory);
        transitionId = match?.id ?? null;
      }
      if (transitionId) {
        this.dialog = {
          kind: 'fields',
          ticket,
          transitionId,
          target: targetName,
          fields: parseFields(e.detail),
          message: e.message,
        };
        return;
      }
    }
    if (e.code === 'not_found' || e.code === 'unsupported') {
      toasts.push({
        level: 'warn',
        text: `No transition to ${targetName} for ${ticket.ref.key}`,
        action: { label: 'Open in browser', command: 'tickets.open_in_browser', args: { url: ticket.url } },
      });
      return;
    }
    toasts.error(err, `Moving ${ticket.ref.key} failed`);
  }
}
