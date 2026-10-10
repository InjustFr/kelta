// Ticket lists (per scope + view), details, columns and transitions. `tickets.changed` invalidates
// and refetches the affected loaded lists.

import type {
  Column,
  ProjectId,
  Scope,
  Ticket,
  TicketDetail,
  TicketItem,
  TicketPage,
  TicketRef,
  Transition,
  UiEvent,
  Who,
} from '$lib/gen';
import * as ipc from '$lib/ipc/commands';

import { idle, settle, type Loadable } from './loadable';
import { scopeAffects, scopeKey } from './reducers';

export interface TicketList extends Loadable<TicketPage> {
  scope: Scope;
  viewId: string | null;
  /** Overrides the view's own `who` (`null` = the view decides). */
  who: Who | null;
  loadingMore: boolean;
}

export function ticketListKey(scope: Scope, viewId: string | null, who: Who | null = null): string {
  return `${scopeKey(scope)}|${viewId ?? ''}|${who ?? ''}`;
}

export function ticketKey(ref: TicketRef): string {
  return `${ref.account}:${ref.key}`;
}

/** Replaces a ticket (matched by ref) inside a page. Returns the same page if absent. */
export function patchTicketInPage(page: TicketPage, ticket: Ticket): TicketPage {
  let changed = false;
  const items = page.items.map((it) => {
    if (it.ticket.ref.account === ticket.ref.account && it.ticket.ref.key === ticket.ref.key) {
      changed = true;
      return { ...it, ticket };
    }
    return it;
  });
  return changed ? { ...page, items } : page;
}

export class TicketsStore {
  lists = $state<Record<string, TicketList>>({});
  details = $state<Record<string, Loadable<TicketDetail>>>({});
  columns = $state<Record<ProjectId, Loadable<Column[]>>>({});
  transitions = $state<Record<string, Loadable<Transition[]>>>({});

  list(scope: Scope, viewId: string | null = null, who: Who | null = null): TicketList {
    return (
      this.lists[ticketListKey(scope, viewId, who)] ?? {
        ...idle<TicketPage>(),
        scope,
        viewId,
        who,
        loadingMore: false,
      }
    );
  }

  items(scope: Scope, viewId: string | null = null, who: Who | null = null): TicketItem[] {
    return this.list(scope, viewId, who).data?.items ?? [];
  }

  async load(
    scope: Scope,
    viewId: string | null = null,
    refresh = false,
    who: Who | null = null,
  ): Promise<TicketList> {
    const key = ticketListKey(scope, viewId, who);
    const prev = this.list(scope, viewId, who);
    this.lists = { ...this.lists, [key]: { ...prev, loading: true } };
    const next = await settle(
      prev,
      () => ipc.trackerList({ scope, view_id: viewId, cursor: null, refresh, who }),
      {
        stale: (p) => p.stale,
      },
    );
    const entry: TicketList = { ...next, scope, viewId, who, loadingMore: false };
    this.lists = { ...this.lists, [key]: entry };
    return entry;
  }

  /** Fetches the next page and appends it. */
  async loadMore(scope: Scope, viewId: string | null = null, who: Who | null = null): Promise<void> {
    const key = ticketListKey(scope, viewId, who);
    const prev = this.lists[key];
    if (!prev?.data?.next || prev.loadingMore) return;
    this.lists = { ...this.lists, [key]: { ...prev, loadingMore: true } };
    try {
      const page = await ipc.trackerList({
        scope,
        view_id: viewId,
        cursor: prev.data.next,
        refresh: false,
        who,
      });
      const cur = this.lists[key] ?? prev;
      // Offset paging can repeat a ticket across pages (order shifted between fetches).
      // eslint-disable-next-line svelte/prefer-svelte-reactivity -- local lookup, not state
      const have = new Set((cur.data?.items ?? []).map((i) => ticketKey(i.ticket.ref)));
      const items = [
        ...(cur.data?.items ?? []),
        ...page.items.filter((i) => !have.has(ticketKey(i.ticket.ref))),
      ];
      this.lists = {
        ...this.lists,
        [key]: {
          ...cur,
          loadingMore: false,
          data: { ...page, items, errors: [...(cur.data?.errors ?? []), ...page.errors] },
        },
      };
    } catch (err) {
      const cur = this.lists[key] ?? prev;
      this.lists = { ...this.lists, [key]: { ...cur, loadingMore: false } };
      throw err;
    }
  }

  async loadDetail(ref: TicketRef): Promise<Loadable<TicketDetail>> {
    const key = ticketKey(ref);
    const prev = this.details[key] ?? idle<TicketDetail>();
    this.details = { ...this.details, [key]: { ...prev, loading: true } };
    const next = await settle(prev, () => ipc.trackerGet({ ticket: ref }));
    this.details = { ...this.details, [key]: next };
    return next;
  }

  async loadColumns(projectId: ProjectId): Promise<Loadable<Column[]>> {
    const prev = this.columns[projectId] ?? idle<Column[]>();
    this.columns = { ...this.columns, [projectId]: { ...prev, loading: true } };
    const next = await settle(prev, () => ipc.trackerColumns({ project_id: projectId }));
    this.columns = { ...this.columns, [projectId]: next };
    return next;
  }

  async loadTransitions(ref: TicketRef): Promise<Loadable<Transition[]>> {
    const key = ticketKey(ref);
    const prev = this.transitions[key] ?? idle<Transition[]>();
    this.transitions = { ...this.transitions, [key]: { ...prev, loading: true } };
    const next = await settle(prev, () => ipc.trackerTransitions({ ticket: ref }));
    this.transitions = { ...this.transitions, [key]: next };
    return next;
  }

  /** Replaces a ticket everywhere (after a mutation or optimistically). */
  patch(ticket: Ticket): void {
    const lists: Record<string, TicketList> = {};
    for (const [k, l] of Object.entries(this.lists)) {
      lists[k] = l.data ? { ...l, data: patchTicketInPage(l.data, ticket) } : l;
    }
    this.lists = lists;
    const key = ticketKey(ticket.ref);
    const d = this.details[key];
    if (d?.data) this.details = { ...this.details, [key]: { ...d, data: { ...d.data, ticket } } };
  }

  /**
   * Optimistic board move: patches the status locally, calls `tracker_move`, applies the result
   * or rolls back (rethrowing the error, e.g. `Conflict` with candidates or `NeedsFields`).
   * `projectId` picks whose columns resolve the move (default: the backend's guess by account).
   */
  async move(ticket: Ticket, column: Column, projectId: ProjectId | null = null): Promise<Ticket> {
    this.patch({
      ...ticket,
      status: { id: `column:${column.id}`, name: column.name, category: column.category },
    });
    try {
      const updated = await ipc.trackerMove({
        ticket: ticket.ref,
        column_id: column.id,
        project_id: projectId,
      });
      this.patch(updated);
      return updated;
    } catch (err) {
      this.patch(ticket);
      throw err;
    }
  }

  apply(ev: UiEvent): void {
    // Not on reviews.changed: reloading every list on each review poll revalidated trackers and dropped
    // loadMore pages; PR chips catch up on the next ticket refresh.
    if (ev.type !== 'tickets.changed') return;
    const lists: Record<string, TicketList> = {};
    let touched = false;
    for (const [k, l] of Object.entries(this.lists)) {
      if (scopeAffects(ev.scope, l.scope)) {
        lists[k] = { ...l, invalidated: true };
        touched = true;
      } else {
        lists[k] = l;
      }
    }
    if (!touched) return;
    this.lists = lists;
    void this.refreshInvalidated();
  }

  /** Refetches every invalidated list that was loaded. */
  async refreshInvalidated(): Promise<void> {
    const stale = Object.values(this.lists).filter((l) => l.invalidated && !l.loading);
    await Promise.all(stale.map((l) => this.load(l.scope, l.viewId, false, l.who)));
  }
}
