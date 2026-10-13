// Tickets list grouping (TICKETS.md T5/T6): To start (Mine's default, #418), Flow, native status, priority, sprint, assignee,
// source, parent or none; sort within groups; status age.

import { msg, t } from '$lib/i18n/index.svelte';
import type {
  Column,
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
  'to_start',
  'status',
  'flow',
  'priority',
  'sprint',
  'assignee',
  'source',
  'parent',
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
  doing: msg('Doing'),
  waiting: msg('Waiting'),
  ready: msg('Ready'),
  backlog: msg('Backlog'),
  done: msg('Done in the last 7 days'),
};
/** What each flow group holds, in one line under its header (#397). */
export const FLOW_HINTS: Record<string, string> = {
  doing: msg('In progress, or work under way in Kelta'),
  waiting: msg('Waiting on you, a review, failed checks or a blocker'),
  ready: msg('To do, ready to start'),
  backlog: msg('Not planned yet'),
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
  if (
    s.needsYou ||
    t.blocked ||
    BLOCKED.test(t.status.name) ||
    (open && (pr.ci === 'failure' || pr.ci === 'error'))
  )
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

// ---- to start (#418) ------------------------------------------------------------------------

export type ToStart = 'up_next' | 'ready' | 'queue' | 'blocked' | 'backlog' | 'done';
const TO_STARTS: readonly ToStart[] = ['up_next', 'ready', 'queue', 'blocked', 'backlog', 'done'];
const TO_START_LABELS: Record<ToStart, string> = {
  up_next: msg('Up next'),
  ready: msg('Ready to start'),
  queue: msg('In the queue'),
  blocked: msg('Blocked'),
  backlog: msg('Backlog'),
  done: msg('Done in the last 7 days'),
};
export const TO_START_HINTS: Record<string, string> = {
  up_next: msg('Your order, as the Queue lists it'),
  ready: msg('No work yet, ready to start'),
  queue: msg('Started: the next step is in the Queue'),
  backlog: msg('Not planned yet'),
};

/** What To start knows beyond the ticket: its unfinished work item and its rank in my Next up list. */
export interface ToStartSignals {
  work: WorkItem | null;
  rank: number | null;
}

/** The To start group of a ticket; `null` = done more than 7 days ago (left out). */
export function toStartOf(item: TicketItem, s: ToStartSignals, now: number): ToStart | null {
  const t = item.ticket;
  if (t.status.category === 'done') return now - since(t) <= 7 * DAY ? 'done' : null;
  // As in the Queue: started tickets leave Next up, blocked ones too.
  if (s.work && s.work.state.kind !== 'finished') return 'queue';
  if (t.blocked || BLOCKED.test(t.status.name)) return 'blocked';
  if (s.rank !== null) return 'up_next';
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

/** Sorted copy, blocked tickets last: priority (then updated), updated (newest), age (longest in status), key (natural). */
export function sortTickets(items: readonly TicketItem[], sort: TicketSort): TicketItem[] {
  const cmp =
    sort === 'priority'
      ? (a: TicketItem, b: TicketItem) => rank(a) - rank(b) || byUpdated(a, b)
      : sort === 'updated'
        ? byUpdated
        : sort === 'age'
          ? (a: TicketItem, b: TicketItem) => since(a.ticket) - since(b.ticket) || byKey(a, b)
          : byKey;
  return [...items].sort((a, b) => Number(a.ticket.blocked) - Number(b.ticket.blocked) || cmp(a, b));
}

// ---- grouping --------------------------------------------------------------------------------

/**
 * Groups in display order, items kept in input order. `sources` maps a source id to its label and gives
 * the source order (a ticket sits under its first source); `flow` classifies for the Flow grouping,
 * `toStart` gives To start each ticket's work and Next up rank.
 * `columns` (in board order) order the Status groups; statuses matching no column by name follow them.
 */
export function groupTickets(
  items: readonly TicketItem[],
  by: GroupBy,
  sources: readonly { id: string; label: string }[] = [],
  flow: (item: TicketItem) => Flow | null = (i) => flowOf(i, { work: null, needsYou: false }, Date.now()),
  columns: readonly Column[] = [],
  toStart: (item: TicketItem) => ToStartSignals = () => ({ work: null, rank: null }),
): TicketGroup[] {
  if (by === 'none') return [{ id: 'all', label: t('All'), category: null, items: [...items] }];
  const groups = new Map<string, TicketGroup & { order: number }>();
  const now = Date.now();
  const ranks = new Map<TicketItem, number>();
  for (const item of items) {
    const tk = item.ticket;
    let g: [string, string, StatusCategory | null, number];
    if (by === 'to_start') {
      const s = toStart(item);
      const f = toStartOf(item, s, now);
      if (!f) continue;
      if (f === 'up_next') ranks.set(item, s.rank!);
      g = [f, t(TO_START_LABELS[f]), f === 'done' ? 'done' : null, TO_STARTS.indexOf(f)];
    } else if (by === 'flow') {
      const f = flow(item);
      if (!f) continue;
      g = [f, t(FLOW_LABELS[f]), f === 'done' ? 'done' : null, FLOWS.indexOf(f)];
    } else if (by === 'status') {
      // By name only: a category match would slot "Open" into the board's Todo column.
      const lower = tk.status.name.toLowerCase();
      const at = columns.findIndex(
        (c) => c.name.toLowerCase() === lower || c.match_names.some((n) => n.toLowerCase() === lower),
      );
      const order = at >= 0 ? at : columns.length + GROUP_ORDER.indexOf(tk.status.category);
      g = [tk.status.name, tk.status.name, tk.status.category, order];
    } else if (by === 'priority') {
      const r = tk.priority_rank;
      g = [
        r === null ? '' : String(r),
        tk.priority ?? (r === null ? t('No priority') : t('Priority {n}', { n: r })),
        null,
        rank(item),
      ];
    } else if (by === 'sprint') {
      // Active sprints first, then the others, then no sprint. Keyed per account: ids repeat across trackers.
      g = [
        tk.sprint ? `${tk.ref.account}:${tk.sprint.id}` : '',
        tk.sprint?.name ?? t('No sprint'),
        null,
        tk.sprint ? (tk.sprint.active ? 0 : 1) : 2,
      ];
    } else if (by === 'assignee') {
      g = [tk.assignee?.id ?? '', tk.assignee?.name ?? t('Unassigned'), null, tk.assignee ? 0 : 1];
    } else if (by === 'parent') {
      // Keyed by URL: keys repeat across trackers. Redmine sends no title: the listed parent's, if any.
      const p = tk.parent;
      const title =
        p &&
        (p.title ||
          items.find((i) => i.ticket.ref.account === tk.ref.account && i.ticket.ref.key === p.key)?.ticket
            .title);
      g = p
        ? [p.url || `${tk.ref.account}:${p.key}`, title ? `${p.key} · ${title}` : p.key, null, 0]
        : ['', t('No parent'), null, 1];
    } else {
      const at = sources.findIndex((s) => s.id === item.source_ids[0]);
      g = [item.source_ids[0] ?? '', sources[at]?.label ?? t('Other'), null, at < 0 ? sources.length : at];
    }
    const [id, label, category, order] = g;
    const group = groups.get(id) ?? { id, label, category, order, items: [] };
    group.items.push(item);
    groups.set(id, group);
  }
  // Up next keeps my order, whatever the sort.
  if (by === 'to_start') groups.get('up_next')?.items.sort((a, b) => ranks.get(a)! - ranks.get(b)!);
  return [...groups.values()]
    .sort((a, b) => a.order - b.order || a.label.localeCompare(b.label))
    .map(({ order: _, ...g }) => g);
}
