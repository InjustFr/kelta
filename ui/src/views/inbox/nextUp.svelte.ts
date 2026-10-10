// Next up (#145): my own cross-project order of tickets to start, snoozes, and the `New` badge
// (tickets I have not looked at yet). Kelta-local (SQLite `next_up`, `seen_tickets`), never written to a
// tracker; the tickets themselves come from the loaded lists (mine + unassigned, every source).

import type { NextUpItem, ProjectId, TicketItem, TicketRef } from '$lib/gen';
import { nextUpList, nextUpPut, nextUpRemove, ticketSeen } from '$lib/ipc/commands';
import { projects, tickets, toasts, work } from '$lib/stores';
import { ticketKey } from '$lib/stores/tickets.svelte';

const ALL = { kind: 'all' } as const;
const DAY_MS = 24 * 3600 * 1000;
export const SNOOZE_DAYS = 7;

// shortcut: plain bisection, precision runs out after ~50 moves into the same gap; renumber if it bites.
/** A rank between two neighbours; either may be missing (an end of the list). */
export function rankBetween(a: number | null, b: number | null): number {
  if (a === null) return b === null ? 0 : b - 1;
  return b === null ? a + 1 : (a + b) / 2;
}

/** The new rank of `ranks[i]` moved one place by `delta`; null at an end. */
export function movedRank(ranks: readonly number[], i: number, delta: 1 | -1): number | null {
  const j = i + delta;
  if (i < 0 || j < 0 || j >= ranks.length) return null;
  // Past the neighbour: between it and the one beyond it.
  return delta > 0
    ? rankBetween(ranks[j]!, ranks[j + 1] ?? null)
    : rankBetween(ranks[j - 1] ?? null, ranks[j]!);
}

export const isSnoozed = (it: NextUpItem | undefined, now: number): boolean =>
  !!it?.snoozed_until && Date.parse(it.snoozed_until) > now;

const isOpen = (t: TicketItem): boolean => t.ticket.status.category !== 'done';

/**
 * The grooming pass: open tickets not started, not in Next up and not snoozed; New ones first, then
 * the pool's order (mine before unassigned).
 */
export function groomQueue(
  pool: readonly TicketItem[],
  entries: Readonly<Record<string, NextUpItem>>,
  isNew: (key: string) => boolean,
  started: (t: TicketItem) => boolean,
  now: number,
): TicketItem[] {
  const open = pool.filter((t) => {
    const e = entries[ticketKey(t.ticket.ref)];
    return isOpen(t) && !started(t) && e?.rank == null && !isSnoozed(e, now);
  });
  const fresh = open.filter((t) => isNew(ticketKey(t.ticket.ref)));
  return [...fresh, ...open.filter((t) => !fresh.includes(t))];
}

/** Mine then unassigned, across every bound tracker, once each. */
export function ticketPool(): TicketItem[] {
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- local dedupe, not state
  const out = new Map<string, TicketItem>();
  for (const who of ['mine', 'unassigned'] as const)
    for (const t of tickets.items(ALL, null, who))
      if (!out.has(ticketKey(t.ticket.ref))) out.set(ticketKey(t.ticket.ref), t);
  return [...out.values()];
}

export async function loadPool(force = false): Promise<void> {
  await Promise.allSettled([
    tickets.load(ALL, null, force, 'mine'),
    tickets.load(ALL, null, force, 'unassigned'),
    nextUp.load(),
  ]);
  await nextUp.baseline();
}

const started = (t: TicketItem): boolean => work.forTicket(t.ticket.ref) !== null;

class NextUpStore {
  entries = $state<Record<string, NextUpItem>>({});
  seen = $state<Record<string, true>>({});
  loaded = $state(false);

  async load(): Promise<void> {
    try {
      const r = await nextUpList({});
      this.entries = Object.fromEntries(r.items.map((i) => [ticketKey(i.ticket), i]));
      this.seen = Object.fromEntries(r.seen.map((k) => [k, true]));
      this.loaded = true;
    } catch (err) {
      toasts.error(err, 'Loading Next up');
    }
  }

  /** Unseen and assigned to me: before the first load nothing is New (no flash of badges). */
  isNewKey(key: string): boolean {
    return (
      this.loaded &&
      !(key in this.seen) &&
      tickets.items(ALL, null, 'mine').some((t) => ticketKey(t.ticket.ref) === key)
    );
  }

  isNew(ref: TicketRef): boolean {
    return this.isNewKey(ticketKey(ref));
  }

  /** Listed or snoozed: left out of Up next and New. */
  parked(ref: TicketRef, now = Date.now()): boolean {
    const e = this.entries[ticketKey(ref)];
    return e !== undefined && (e.rank !== null || isSnoozed(e, now));
  }

  /** The list in order, snoozed ones left out. */
  ordered(now = Date.now()): NextUpItem[] {
    return Object.values(this.entries)
      .filter((e) => e.rank !== null && !isSnoozed(e, now))
      .sort((a, b) => a.rank! - b.rank!);
  }

  /** `n` (bottom) / `N` (top). */
  add(ref: TicketRef, projectId: ProjectId | null, top: boolean): Promise<void> {
    const key = ticketKey(ref);
    const ranks = Object.values(this.entries).flatMap((e) =>
      e.rank === null || ticketKey(e.ticket) === key ? [] : [e.rank],
    );
    const rank = ranks.length === 0 ? 0 : top ? Math.min(...ranks) - 1 : Math.max(...ranks) + 1;
    this.markSeen(ref);
    return this.#put({ project_id: projectId ?? '', ticket: ref, rank, snoozed_until: null });
  }

  /** `l`: hidden from grooming (and Next up when listed) for a week. */
  snooze(ref: TicketRef, projectId: ProjectId | null, days = SNOOZE_DAYS): Promise<void> {
    const until = new Date(Date.now() + days * DAY_MS).toISOString();
    this.markSeen(ref);
    return this.#put({
      project_id: projectId ?? '',
      ticket: ref,
      rank: this.entries[ticketKey(ref)]?.rank ?? null,
      snoozed_until: until,
    });
  }

  /** `J` / `K` on a Next up row: one place down or up in the list as shown. */
  move(ref: TicketRef, delta: 1 | -1, shown: readonly TicketRef[]): Promise<void> {
    const ranks = shown.map((r) => this.entries[ticketKey(r)]?.rank ?? 0);
    const rank = movedRank(
      ranks,
      shown.findIndex((r) => ticketKey(r) === ticketKey(ref)),
      delta,
    );
    const e = this.entries[ticketKey(ref)];
    return rank === null || !e ? Promise.resolve() : this.#put({ ...e, rank });
  }

  async remove(ref: TicketRef): Promise<void> {
    const { [ticketKey(ref)]: gone, ...rest } = this.entries;
    if (!gone) return;
    this.entries = rest;
    await nextUpRemove({ ticket: ref }).catch((err) => toasts.error(err, `Removing ${ref.key} from Next up`));
  }

  /** Clears the `New` badge (groomed or opened). */
  markSeen(ref: TicketRef): void {
    const key = ticketKey(ref);
    if (key in this.seen) return;
    this.seen = { ...this.seen, [key]: true };
    ticketSeen({ tickets: [ref] }).catch((err) => toasts.error(err, `Marking ${ref.key} seen`));
  }

  /**
   * First run (nothing seen yet): every ticket already assigned to me counts as seen, so `New` means
   * assigned since then, not everything open on the day of the upgrade (unassigned ones are never New).
   */
  async baseline(): Promise<void> {
    const mine = tickets.list(ALL, null, 'mine');
    if (!this.loaded || Object.keys(this.seen).length || !mine.data || mine.error) return;
    // shortcut: with no ticket loaded yet no baseline is set, so the first ones to arrive later count as seen.
    const refs = mine.data.items.map((t) => t.ticket.ref);
    if (!refs.length) return;
    this.seen = Object.fromEntries(refs.map((r) => [ticketKey(r), true]));
    await ticketSeen({ tickets: refs }).catch((err) => toasts.error(err, 'Marking tickets seen'));
  }

  /**
   * The Now refresh, after the lists load: drops the items whose ticket closed or was started, and,
   * when both lists came back whole (no error, no further page), the ones neither list has any more
   * (closed in a view that hides done tickets, or given to someone else). The lists only cover open
   * projects, so an item of a closed project is kept.
   */
  async prune(): Promise<void> {
    const lists = (['mine', 'unassigned'] as const).map((w) => tickets.list(ALL, null, w));
    const whole = lists.every((l) => l.data && !l.error && !l.stale && !l.data.next && !l.data.errors.length);
    const pool = new Map(ticketPool().map((t) => [ticketKey(t.ticket.ref), t]));
    for (const [key, e] of Object.entries(this.entries)) {
      if (e.rank === null) continue;
      const t = pool.get(key);
      if (t ? !isOpen(t) || started(t) : whole && projects.openProjects.some((p) => p.id === e.project_id))
        await this.remove(e.ticket);
    }
  }

  async #put(item: NextUpItem): Promise<void> {
    this.entries = { ...this.entries, [ticketKey(item.ticket)]: item };
    try {
      await nextUpPut({ item });
    } catch (err) {
      toasts.error(err, `Saving ${item.ticket.key} in Next up`);
      await this.load();
    }
  }
}

export const nextUp = new NextUpStore();
