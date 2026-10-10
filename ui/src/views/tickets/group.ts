// Tickets list grouping (TICKETS.md T5/T6): Flow (default), native status, priority, sprint, assignee,
// source or none; sort within groups; status age.

import type {
  Status,
  StatusCategory,
  Ticket,
  TicketGroupBy,
  TicketItem,
  TicketSort,
  WorkItem,
} from '$lib/gen';

import { mainPr } from './prs';

export type GroupBy = TicketGroupBy;
export const GROUP_BYS: readonly GroupBy[] = [
  'flow',
  'status',
  'priority',
  'sprint',
  'assignee',
  'source',
  'none',
];
export const SORTS: readonly TicketSort[] = ['priority', 'updated', 'age', 'key'];

export interface TicketGroup {
  id: string;
  label: string;
  /** The category that orders and colours the group; `done` starts collapsed. */
  category: StatusCategory | null;
  items: TicketItem[];
}

/** Attention order for groups: what moves first, done last. */
const GROUP_ORDER: StatusCategory[] = ['in_progress', 'in_review', 'todo', 'unknown', 'done'];
/** Workflow order for the move menu's flow strip. */
const FLOW_ORDER: StatusCategory[] = ['todo', 'in_progress', 'in_review', 'done', 'unknown'];

/** 2px category bar colour (DESIGN: the category colours, the native name labels). */
export function categoryBar(c: StatusCategory): string {
  return c === 'in_progress'
    ? 'var(--k-info)'
    : c === 'in_review'
      ? 'var(--k-warn)'
      : c === 'done'
        ? 'var(--k-ok)'
        : 'var(--k-border-strong)';
}

/** Statuses deduped by name, in workflow order (stable within a category). */
export function flowOrder(statuses: Status[]): Status[] {
  const seen = new Map<string, Status>();
  // "To Do" (the ticket) and "To do" (a transition) are one step; the first spelling wins.
  for (const s of statuses) if (!seen.has(s.name.toLowerCase())) seen.set(s.name.toLowerCase(), s);
  return [...seen.values()].sort((a, b) => FLOW_ORDER.indexOf(a.category) - FLOW_ORDER.indexOf(b.category));
}

// ---- flow ------------------------------------------------------------------------------------

export type Flow = 'doing' | 'waiting' | 'ready' | 'backlog' | 'done';
const FLOWS: readonly Flow[] = ['doing', 'waiting', 'ready', 'backlog', 'done'];
const FLOW_LABELS: Record<Flow, string> = {
  doing: 'Doing',
  waiting: 'Waiting',
  ready: 'Ready',
  backlog: 'Backlog',
  done: 'Done in the last 7 days',
};
const BLOCKED = /block|on hold|waiting|impediment/i;
const BACKLOG = /backlog|triage|icebox/i;
const DAY = 86_400_000;

/** What the flow grouping knows beyond the ticket: its local work item and whether Claude waits on me. */
export interface FlowSignals {
  work: WorkItem | null;
  needsYou: boolean;
}

/** The flow group of a ticket; `null` = done more than 7 days ago (left out of Flow). */
export function flowOf(item: TicketItem, s: FlowSignals, now: number): Flow | null {
  const t = item.ticket;
  if (t.status.category === 'done') return now - since(t) <= 7 * DAY ? 'done' : null;
  const pr = mainPr(item.prs);
  const open = pr?.state === 'open';
  if (s.needsYou || BLOCKED.test(t.status.name) || (open && (pr.ci === 'failure' || pr.ci === 'error')))
    return 'waiting';
  if (
    s.work?.state.kind === 'active' ||
    s.work?.state.kind === 'starting' ||
    t.status.category === 'in_progress'
  )
    return 'doing';
  if (t.status.category === 'in_review' || (open && !pr.draft && pr.review === 'review_required'))
    return 'waiting';
  if (t.status.category === 'unknown' || BACKLOG.test(t.status.name)) return 'backlog';
  return 'ready';
}

// ---- age and sort ----------------------------------------------------------------------------

/** When the ticket entered its status (ms); providers without it fall back to `updated_at`. */
function since(t: Ticket): number {
  return Date.parse(t.status_since ?? t.updated_at) || 0;
}

/** Whole days in the current status. */
export function ageDays(t: Ticket, now: number): number {
  return Math.max(0, Math.floor((now - since(t)) / DAY));
}

/** Age badge tone at 7 / 14 / 21 days in a non-done status; `null` = no badge. */
export function ageLevel(t: Ticket, now: number): 'old' | 'warn' | 'danger' | null {
  if (t.status.category === 'done') return null;
  const d = ageDays(t, now);
  return d >= 21 ? 'danger' : d >= 14 ? 'warn' : d >= 7 ? 'old' : null;
}

const byKey = (a: TicketItem, b: TicketItem): number =>
  a.ticket.ref.key.localeCompare(b.ticket.ref.key, undefined, { numeric: true });
const byUpdated = (a: TicketItem, b: TicketItem): number =>
  Date.parse(b.ticket.updated_at) - Date.parse(a.ticket.updated_at);
/** No priority sorts after every rank. */
const rank = (i: TicketItem): number => i.ticket.priority_rank ?? 256;

/** Sorted copy: priority (then updated), updated (newest), age (longest in status), key (natural). */
export function sortTickets(items: readonly TicketItem[], sort: TicketSort): TicketItem[] {
  const cmp =
    sort === 'priority'
      ? (a: TicketItem, b: TicketItem) => rank(a) - rank(b) || byUpdated(a, b)
      : sort === 'updated'
        ? byUpdated
        : sort === 'age'
          ? (a: TicketItem, b: TicketItem) => since(a.ticket) - since(b.ticket) || byKey(a, b)
          : byKey;
  return [...items].sort(cmp);
}

// ---- grouping --------------------------------------------------------------------------------

/**
 * Groups in display order, items kept in input order. `sources` maps a view id to its label and gives
 * the source order (a ticket sits under its first view); `flow` classifies for the Flow grouping.
 */
export function groupTickets(
  items: readonly TicketItem[],
  by: GroupBy,
  sources: readonly { id: string; label: string }[] = [],
  flow: (item: TicketItem) => Flow | null = (i) => flowOf(i, { work: null, needsYou: false }, Date.now()),
): TicketGroup[] {
  if (by === 'none') return [{ id: 'all', label: 'All', category: null, items: [...items] }];
  const groups = new Map<string, TicketGroup & { order: number }>();
  for (const item of items) {
    const t = item.ticket;
    let g: [string, string, StatusCategory | null, number];
    if (by === 'flow') {
      const f = flow(item);
      if (!f) continue;
      g = [f, FLOW_LABELS[f], f === 'done' ? 'done' : null, FLOWS.indexOf(f)];
    } else if (by === 'status') {
      g = [t.status.name, t.status.name, t.status.category, GROUP_ORDER.indexOf(t.status.category)];
    } else if (by === 'priority') {
      const r = t.priority_rank;
      g = [
        r === null ? '' : String(r),
        t.priority ?? (r === null ? 'No priority' : `Priority ${r}`),
        null,
        rank(item),
      ];
    } else if (by === 'sprint') {
      // Active sprints first, then the others, then no sprint. Keyed per account: ids repeat across trackers.
      g = [
        t.sprint ? `${t.ref.account}:${t.sprint.id}` : '',
        t.sprint?.name ?? 'No sprint',
        null,
        t.sprint ? (t.sprint.active ? 0 : 1) : 2,
      ];
    } else if (by === 'assignee') {
      g = [t.assignee?.id ?? '', t.assignee?.name ?? 'Unassigned', null, t.assignee ? 0 : 1];
    } else {
      const at = sources.findIndex((s) => s.id === item.view_ids[0]);
      g = [item.view_ids[0] ?? '', sources[at]?.label ?? 'Other', null, at < 0 ? sources.length : at];
    }
    const [id, label, category, order] = g;
    const group = groups.get(id) ?? { id, label, category, order, items: [] };
    group.items.push(item);
    groups.set(id, group);
  }
  return [...groups.values()]
    .sort((a, b) => a.order - b.order || a.label.localeCompare(b.label))
    .map(({ order: _, ...g }) => g);
}
