// Now's sections (FLOW §3.1): one row per thing, a work item once (in its phase's section),
// sections in the order to work in, each with its own order. Pure: the pane and the rail feed it.

import type { ReviewItem, SessionInfo, TicketItem, WorkItem } from '$lib/gen';

import { reviewPhase } from '../work/common';
import type { Lamp, NowSection, Phase, PhaseId } from '../work/phase';

export interface WorkEntry {
  item: WorkItem;
  phase: Phase;
}

export type NowRow =
  | { type: 'work'; id: string; item: WorkItem; phase: Phase }
  | { type: 'session'; id: string; session: SessionInfo }
  | { type: 'review'; id: string; review: ReviewItem; mine: boolean; reason: string }
  | { type: 'ticket'; id: string; ticket: TicketItem };

export interface NowInput {
  /** Unfinished work items with their phase. */
  work: readonly WorkEntry[];
  sessions: readonly SessionInfo[];
  requested: readonly ReviewItem[];
  authored: readonly ReviewItem[];
  /** Tickets assigned to me, in tracker order. */
  tickets: readonly TicketItem[];
}

export interface Section {
  id: NowSection;
  label: string;
  lamp: Lamp;
  rows: NowRow[];
  /** Rows left out by the cap (Up next shows 10, then "Show all on Board"). */
  more: number;
}

export const SECTIONS: readonly { id: NowSection; label: string; lamp: Lamp }[] = [
  { id: 'needs_you', label: 'Claude needs you', lamp: 'needs_input' },
  { id: 'to_review', label: 'Ready for review', lamp: 'done' },
  { id: 'fix', label: 'Fix', lamp: 'error' },
  { id: 'requests', label: 'Review requests', lamp: 'none' },
  { id: 'ship', label: 'Ship and clean up', lamp: 'none' },
  { id: 'in_flight', label: 'In flight', lamp: 'working' },
  { id: 'up_next', label: 'Up next', lamp: 'none' },
];

/** The first four sections: what waits on Louis (badge, Next waiting). */
export const WAITING: readonly NowSection[] = ['needs_you', 'to_review', 'fix', 'requests'];

export const UP_NEXT_MAX = 10;

// Order within a section: phases listed first come first.
const PHASE_ORDER: Partial<Record<PhaseId, number>> = {
  changes_requested: 0,
  checks_failed: 1,
  remote_new: 2,
  rebase_stopped: 3,
  conflicts: 3,
  failed: 4,
  approved: 0,
  unpushed: 1,
  rebased: 1,
  ready: 2,
  merged: 3,
  closed: 3,
  reviewed: 4,
  working: 0,
  starting: 0,
  in_review: 1,
  no_changes: 2,
  missing: 2,
};

const prKey = (r: ReviewItem): string =>
  `${r.review.ref.account}:${r.review.ref.repo}#${r.review.ref.number}`;
const ticketKey = (t: TicketItem): string => `${t.ticket.ref.account}:${t.ticket.ref.key}`;

/** Builds the seven sections; empty ones are dropped. */
export function nowSections(input: NowInput): Section[] {
  // `size` breaks ties: a smaller delta to review first.
  type Ranked = { row: NowRow; rank: number; at: string; size: number };
  const by = new Map<NowSection, Ranked[]>(SECTIONS.map((s) => [s.id, []]));
  const add = (section: NowSection, row: NowRow, rank: number, at: string, size = 0) =>
    by.get(section)!.push({ row, rank, at, size });

  const itemIds = new Set(input.work.map((w) => w.item.id));
  for (const { item, phase } of input.work)
    add(
      phase.section,
      { type: 'work', id: `w:${item.id}`, item, phase },
      PHASE_ORDER[phase.id] ?? 0,
      item.claude_at ?? item.created_at,
      item.delta ? item.delta.lines + item.delta.generated : 0,
    );

  // Plain sessions asking (WaitingUser is idle, never "needs you").
  for (const s of input.sessions)
    if (s.status === 'needs_input' && !(s.work_item_id && itemIds.has(s.work_item_id)))
      add('needs_you', { type: 'session', id: `s:${s.id}`, session: s }, 0, s.created_at);

  // PRs a work item already stands for are not listed twice.
  const ownedPrs = new Set<string>();
  for (const { item } of input.work) {
    if (item.review) ownedPrs.add(`${item.review.account}:${item.review.repo}#${item.review.number}`);
    for (const a of input.authored)
      if (
        a.review.url === item.pr_url ||
        (a.review.source_branch === item.branch && a.project_ids.includes(item.project_id))
      )
        ownedPrs.add(prKey(a));
  }
  for (const r of input.requested) {
    const rp = reviewPhase(r.review);
    if (ownedPrs.has(prKey(r)) || rp === 'reviewed') continue;
    const reason = rp === 'updated' ? 'Updated since your review' : 'Review requested';
    add(
      'requests',
      { type: 'review', id: `r:${prKey(r)}`, review: r, mine: false, reason },
      0,
      r.review.updated_at,
    );
  }
  for (const a of input.authored) {
    if (ownedPrs.has(prKey(a))) continue;
    const reason =
      a.review.decision === 'changes_requested'
        ? 'Changes requested'
        : a.review.ci === 'failure' || a.review.ci === 'error'
          ? 'Checks failed'
          : null;
    if (reason)
      add(
        'fix',
        { type: 'review', id: `p:${prKey(a)}`, review: a, mine: true, reason },
        reason === 'Changes requested' ? 0 : 1,
        a.review.updated_at,
      );
    else
      add(
        'in_flight',
        { type: 'review', id: `p:${prKey(a)}`, review: a, mine: true, reason: 'In review' },
        1,
        a.review.updated_at,
      );
  }

  const started = new Set(
    input.work.flatMap(({ item }) => (item.ticket ? [`${item.ticket.account}:${item.ticket.key}`] : [])),
  );
  input.tickets
    .filter((t) => t.ticket.status.category !== 'done' && !started.has(ticketKey(t)))
    .forEach((t, i) =>
      add(
        'up_next',
        { type: 'ticket', id: `t:${ticketKey(t)}`, ticket: t },
        t.ticket.status.category === 'in_progress' ? 0 : 1,
        String(i).padStart(6, '0'),
      ),
    );

  // Oldest first, except In flight (latest activity first). Up next keeps tracker order (`at` = index).
  // Work items are aged by when Claude last asked or stopped (`claude_at`), else when they started.
  const newestFirst = new Set<NowSection>(['in_flight']);
  return SECTIONS.flatMap((s) => {
    const ranked = by
      .get(s.id)!
      .sort(
        (a, b) =>
          a.rank - b.rank ||
          (newestFirst.has(s.id) ? b.at.localeCompare(a.at) : a.at.localeCompare(b.at)) ||
          a.size - b.size,
      );
    if (ranked.length === 0) return [];
    const cap = s.id === 'up_next' ? UP_NEXT_MAX : Infinity;
    return [{ ...s, rows: ranked.slice(0, cap).map((r) => r.row), more: Math.max(0, ranked.length - cap) }];
  });
}

const count = (sections: readonly Section[], id: NowSection): number => {
  const s = sections.find((x) => x.id === id);
  return s ? s.rows.length + s.more : 0;
};

/** Rail badge: what waits on Louis (the first four sections). */
export function waitingCount(sections: readonly Section[]): number {
  return WAITING.reduce((n, id) => n + count(sections, id), 0);
}

/**
 * The split header in decision order, zero parts dropped:
 * `Claude: 1 asks`, `3 LLM diffs`, `2 PRs waiting`, `Fix 2`, `1 working`, `4 up next`.
 */
export function headerParts(sections: readonly Section[], working: number): string[] {
  const asks = count(sections, 'needs_you');
  const ready = count(sections, 'to_review');
  const prs = count(sections, 'requests');
  const fix = count(sections, 'fix');
  const next = count(sections, 'up_next');
  return [
    asks ? `Claude: ${asks} asks` : '',
    ready ? `${ready} LLM diff${ready === 1 ? '' : 's'}` : '',
    prs ? `${prs} PR${prs === 1 ? '' : 's'} waiting` : '',
    fix ? `Fix ${fix}` : '',
    working ? `${working} working` : '',
    next ? `${next} up next` : '',
  ].filter(Boolean);
}

/** Ready for review rows per project (the project rail's count badge). */
export function readyByProject(sections: readonly Section[]): Map<string, number> {
  const out = new Map<string, number>();
  for (const r of sections.find((s) => s.id === 'to_review')?.rows ?? [])
    if (r.type === 'work') out.set(r.item.project_id, (out.get(r.item.project_id) ?? 0) + 1);
  return out;
}
