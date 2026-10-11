// Finish before starting (#354): starting a new ticket lists my unfinished work first,
// closest to done first. It asks, it never forbids.

import type { StartWorkPlan, TicketItem, WorkItem } from '$lib/gen';
import { settings, tickets } from '$lib/stores';
import { ticketKey } from '$lib/stores/tickets.svelte';

import { phaseNow, prOf, unfinishedWork, workTitle } from './live';
import { workKey, type Phase } from './phase';

export interface Unfinished {
  key: string;
  title: string;
  /** The work item's phase, else the ticket's status. */
  phase: string;
  blocked: boolean;
  /** Exactly one of `item` / `ticket`. */
  item: WorkItem | null;
  ticket: TicketItem | null;
}

export interface WorkRow {
  item: WorkItem;
  phase: Phase;
  title: string;
  hasPr: boolean;
}

/** Merged, approved, in review, then in progress. */
function workRank(w: WorkRow): number {
  if (['merged', 'closed', 'reviewed'].includes(w.phase.id)) return 0;
  if (w.phase.id === 'approved') return 1;
  return w.hasPr || w.item.kind === 'review' ? 2 : 3;
}

/** My unfinished work items, then my in-progress / in-review tickets that have none, ordered
 *  closest to done first; `starting` (a ticket key) is left out. Blocked tickets stay in. */
export function orderUnfinished(work: WorkRow[], mine: TicketItem[], starting: string): Unfinished[] {
  const byKey = new Map(mine.map((t) => [ticketKey(t.ticket.ref), t]));
  const covered = new Set(work.flatMap((w) => (w.item.ticket ? [ticketKey(w.item.ticket)] : [])));
  const others = work.filter((w) => !w.item.ticket || ticketKey(w.item.ticket) !== starting);
  const rows: (Unfinished & { rank: number })[] = others.map((w) => ({
    key: workKey(w.item),
    title: w.title,
    phase: w.phase.label,
    blocked: !!(w.item.ticket && byKey.get(ticketKey(w.item.ticket))?.ticket.blocked),
    item: w.item,
    ticket: null,
    rank: workRank(w),
  }));
  for (const t of mine) {
    const key = ticketKey(t.ticket.ref);
    const cat = t.ticket.status.category;
    if (key === starting || covered.has(key) || (cat !== 'in_progress' && cat !== 'in_review')) continue;
    rows.push({
      key: t.ticket.ref.key,
      title: t.ticket.title,
      phase: t.ticket.status.name,
      blocked: t.ticket.blocked,
      item: null,
      ticket: t,
      rank: cat === 'in_review' ? 2 : 3,
    });
  }
  return rows.sort((a, b) => a.rank - b.rank).map(({ rank: _, ...r }) => r);
}

/** What the plan sheet lists before a new ticket start; empty for a resume, another source or
 *  `work.finish_first = false`. */
export function finishFirst(plan: StartWorkPlan): Unfinished[] {
  if (plan.existing !== null || plan.source.kind !== 'ticket') return [];
  if (settings.value(plan.project_id)?.work.finish_first === false) return [];
  const work = unfinishedWork().map((item) => ({
    item,
    phase: phaseNow(item),
    title: workTitle(item),
    hasPr: item.pr_url !== null || prOf(item) !== null,
  }));
  // shortcut: only the loaded Mine list (Now and the rail load it); a later page is not listed.
  const mine = tickets.items({ kind: 'all' }, null, 'mine');
  return orderUnfinished(work, mine, ticketKey(plan.source.ticket));
}
