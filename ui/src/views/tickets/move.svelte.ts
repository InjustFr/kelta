// Ticket moves (SPEC §3.4): optimistic board move with rollback, ambiguous-transition picker,
// NeedsFields form and the "no transition" toast.

import type { Column, JsonValue, ProjectId, Ticket, Transition } from '$lib/gen';
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

  /** Board/list move to a column of `projectId`'s board: optimistic, rolled back by the store on any error. */
  async moveToColumn(ticket: Ticket, column: Column, projectId: ProjectId | null = null): Promise<boolean> {
    if (this.busy) return false;
    this.busy = true;
    try {
      await tickets.move(ticket, column, projectId);
      return true;
    } catch (err) {
      await this.#handle(err, ticket, column.name, column.category, null);
      return false;
    } finally {
      this.busy = false;
    }
  }

  /** "Move to…" from the detail: applies one transition (shows the form when fields are needed). */
  async moveViaTransition(
    ticket: Ticket,
    transition: Transition,
    fields?: JsonValue,
    quiet = false,
  ): Promise<boolean> {
    if (this.busy) return false;
    this.busy = true;
    try {
      const updated = await ipc.trackerTransition({
        ticket: ticket.ref,
        transition_id: transition.id,
        fields: fields ?? null,
      });
      tickets.patch(updated);
      void tickets.loadTransitions(ticket.ref); // the legal moves changed with the status
      this.dialog = null;
      if (!quiet) toasts.info(`Moved ${ticket.ref.key} to ${transition.to.name}`);
      return true;
    } catch (err) {
      await this.#handle(err, ticket, transition.to.name, transition.to.category, transition);
      return false;
    } finally {
      this.busy = false;
    }
  }

  /**
   * Status picker move, one ticket or a selection, in order.
   * shortcut: stops at the first ticket that fails or asks for fields (its dialog opens), the rest
   * stay where they are and a toast says how many moved; a bulk fields form would lift this.
   */
  async moveAll(moves: { ticket: Ticket; transition: Transition }[]): Promise<void> {
    let done = 0;
    // A bulk move toasts once at the end, not per ticket.
    const quiet = moves.length > 1;
    for (const m of moves) {
      if (!(await this.moveViaTransition(m.ticket, m.transition, undefined, quiet))) break;
      done++;
    }
    const to = moves[0]?.transition.to.name;
    if (moves.length < 2 || !to) return;
    if (done === moves.length) toasts.info(`Moved ${done} tickets to ${to}`);
    else toasts.warn(`Moved ${done} of ${moves.length} tickets to ${to}`);
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
      void tickets.loadTransitions(d.ticket.ref);
      this.dialog = null;
      toasts.info(`Moved ${d.ticket.ref.key} to ${d.target}`);
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
    // The tracker's own words (workflow validator, 422...) and the way out: its web UI.
    toasts.push({
      level: 'error',
      text: `Moving ${ticket.ref.key} failed: ${e.message}`,
      action: { label: 'Open in browser', command: 'tickets.open_in_browser', args: { url: ticket.url } },
    });
  }
}
