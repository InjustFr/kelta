// Now's sections (FLOW §3.1): one row per thing, a work item once (in its phase's section),
// sections in the order to work in, each with its own order. Pure: the pane and the rail feed it.

import { msg, t, tn } from '$lib/i18n/index.svelte';
import type { OutsideSession, ReviewItem, SessionInfo, TicketItem, WorkItem } from '$lib/gen';

import { reviewPhase } from '../work/common';
import { questionOf, workKey, type Lamp, type NowSection, type Phase, type PhaseId } from '../work/phase';

export interface WorkEntry {
  item: WorkItem;
  phase: Phase;
}

export type NowRow =
  /** `blocked`: its ticket is blocked, so it sits in Blocked whatever its phase (#390).
   *  `status`: its ticket's waiting status, in Waiting to deploy (#391). */
  | { type: 'work'; id: string; item: WorkItem; phase: Phase; blocked?: boolean; status?: string }
  | { type: 'session'; id: string; session: SessionInfo }
  /** A Claude session Kelta did not start (#345). */
  | { type: 'outside'; id: string; session: OutsideSession }
  | { type: 'review'; id: string; review: ReviewItem; mine: boolean; reason: string }
  | { type: 'ticket'; id: string; ticket: TicketItem };

export interface NowInput {
  /** Unfinished work items with their phase. */
  work: readonly WorkEntry[];
  sessions: readonly SessionInfo[];
  /** Claude sessions Kelta did not start: asking ones in Claude needs you, the others in Elsewhere. */
  outside?: readonly OutsideSession[];
  requested: readonly ReviewItem[];
  authored: readonly ReviewItem[];
  /** Tickets assigned to me, in tracker order. */
  tickets: readonly TicketItem[];
  /** My Next up list, in its order (#145). */
  nextUp?: readonly TicketItem[];
  /** Ticket keys (`account:key`) left out of Up next: listed in Next up or snoozed. */
  parked?: ReadonlySet<string>;
  /** Ticket keys assigned to me I have not looked at yet (`New for you`). */
  fresh?: ReadonlySet<string>;
  /** Source id → its `waiting_statuses`: tickets there go to Waiting to deploy (#378, #391). */
  waiting?: Waiting;
}

export type Waiting = ReadonlyMap<string, readonly string[]>;

/** In a status its source lists as waiting (#378): done as far as Now and grooming go. */
export const waits = (t: TicketItem, waiting?: Waiting): boolean =>
  t.source_ids.some((id) => waiting?.get(id)?.includes(t.ticket.status.name));

export interface Section {
  id: NowSection;
  label: string;
  lamp: Lamp;
  rows: NowRow[];
  /** Rows left out by the cap (Up next shows 10, then "Show all"). */
  more: number;
  /** Starts collapsed; its header opens it. */
  folded?: boolean;
}

/** What each section holds and why, in one line after its header (#397). */
export const SECTION_HINTS: Record<NowSection, string> = {
  needs_you: msg('Claude stopped and is waiting for your answer'),
  to_review: msg('Claude finished: look at the changes, then ship or ask for more'),
  next_up: msg('Tickets you lined up, in your order; drag to reorder'),
  new: msg('Tickets assigned to you that you have not opened yet'),
  fix: msg('Failed checks, requested changes, conflicts or failed steps'),
  requests: msg('Pull requests waiting for your review'),
  ship: msg('Approved or ready: push, merge or clean up'),
  in_flight: msg('Your other work under way'),
  elsewhere: msg('Claude sessions Kelta did not start'),
  blocked: msg('Waiting on another ticket or team'),
  up_next: msg('Tickets assigned to you that nobody started'),
  waiting: msg('Done on your side, waiting for a deploy'),
};

export const SECTIONS: readonly { id: NowSection; label: string; lamp: Lamp; folded?: boolean }[] = [
  { id: 'needs_you', label: msg('Claude needs you'), lamp: 'needs_input' },
  { id: 'to_review', label: msg('Ready for review'), lamp: 'done' },
  { id: 'next_up', label: msg('Next up'), lamp: 'none' },
  { id: 'new', label: msg('New for you'), lamp: 'none' },
  { id: 'fix', label: msg('Fix'), lamp: 'error' },
  { id: 'requests', label: msg('Review requests'), lamp: 'none' },
  { id: 'ship', label: msg('Ship and clean up'), lamp: 'none' },
  { id: 'in_flight', label: msg('In flight'), lamp: 'working' },
  { id: 'elsewhere', label: msg('Elsewhere'), lamp: 'none' },
  { id: 'blocked', label: msg('Blocked'), lamp: 'none' },
  { id: 'up_next', label: msg('Up next'), lamp: 'none' },
  { id: 'waiting', label: msg('Waiting to deploy'), lamp: 'none', folded: true },
];

/** The first four sections: what waits on Louis (badge, Next waiting). */
export const WAITING: readonly NowSection[] = ['needs_you', 'to_review', 'fix', 'requests'];

export const UP_NEXT_MAX = 10;

/** Sections a work item keeps even when its ticket is blocked (#390). */
const NEEDS_ME = new Set<NowSection>(['needs_you', 'fix', 'to_review']);

// Order within a section: phases listed first come first.
const PHASE_ORDER: Partial<Record<PhaseId, number>> = {
  changes_requested: 0,
  new_comments: 0,
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

/** A row of the Next up section (`s` starts it, `S` the top ones, `J`/`K` reorder). */
export const isNextUp = (row: NowRow): row is Extract<NowRow, { type: 'ticket' }> =>
  row.type === 'ticket' && row.id.startsWith('n:');

/** Builds the sections; empty ones are dropped. */
export function nowSections(input: NowInput): Section[] {
  // `size` breaks ties: a smaller delta to review first.
  type Ranked = { row: NowRow; rank: number; at: string; size: number };
  const by = new Map<NowSection, Ranked[]>(SECTIONS.map((s) => [s.id, []]));
  const add = (section: NowSection, row: NowRow, rank: number, at: string, size = 0) =>
    by.get(section)!.push({ row, rank, at, size });

  // A ticket in its source's waiting status, not done: Waiting to deploy, nowhere else (#391).
  const deploying = (t: TicketItem | undefined): t is TicketItem =>
    !!t && t.ticket.status.category !== 'done' && waits(t, input.waiting);
  // shortcut: a work item's ticket is found only among mine and Next up; pass the whole pool if one is missed.
  const known = new Map([...(input.nextUp ?? []), ...input.tickets].map((t) => [ticketKey(t), t]));

  const itemIds = new Set(input.work.map((w) => w.item.id));
  for (const { item, phase } of input.work) {
    const tk = item.ticket ? known.get(`${item.ticket.account}:${item.ticket.key}`) : undefined;
    // Claude needs you and Fix keep their place: they still need me. Waiting wins over blocked (my part is done).
    const waiting = deploying(tk) && phase.section !== 'needs_you' && phase.section !== 'fix';
    // A blocked ticket's work waits in Blocked unless it needs me now (a question, red CI, a diff).
    const held = !waiting && !!tk?.ticket.blocked && !NEEDS_ME.has(phase.section);
    add(
      waiting ? 'waiting' : held ? 'blocked' : phase.section,
      {
        type: 'work',
        id: `w:${item.id}`,
        item,
        phase,
        ...(waiting ? { status: tk.ticket.status.name } : held ? { blocked: true } : {}),
      },
      PHASE_ORDER[phase.id] ?? 0,
      item.claude_at ?? item.created_at,
      item.delta ? item.delta.lines + item.delta.generated : 0,
    );
  }

  // Plain sessions asking (WaitingUser is idle, never "needs you"), or with an `ask` question (#356);
  // a listed item's run session shows its question on that item's row (a permission prompt stays a row here).
  const runIds = new Set(input.work.map((w) => w.item.run?.session_id));
  for (const s of input.sessions)
    if (
      !(s.work_item_id && itemIds.has(s.work_item_id)) &&
      (questionOf(s) ? !runIds.has(s.id) : s.status === 'needs_input')
    )
      add('needs_you', { type: 'session', id: `s:${s.id}`, session: s }, 0, s.created_at);
  for (const s of input.outside ?? [])
    add(
      s.status === 'needs_input' ? 'needs_you' : 'elsewhere',
      { type: 'outside', id: `o:${s.id}`, session: s },
      1,
      s.updated_at,
    );

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
    // The blocking band first (Louis is the last required reviewer), then the oldest request.
    const reason = r.review.blocking
      ? msg("Blocking: you're the last reviewer")
      : rp === 'updated'
        ? msg('Updated since your review')
        : msg('Review requested');
    add(
      'requests',
      { type: 'review', id: `r:${prKey(r)}`, review: r, mine: false, reason },
      r.review.blocking ? 0 : 1,
      r.review.requested_at ?? r.review.updated_at,
    );
  }
  for (const a of input.authored) {
    // Assigned PRs (someone else's) are Mine rows only.
    if (a.review.assigned || ownedPrs.has(prKey(a))) continue;
    const reason =
      a.review.decision === 'changes_requested'
        ? msg('Changes requested')
        : a.review.ci === 'failure' || a.review.ci === 'error'
          ? msg('Checks failed')
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
        { type: 'review', id: `p:${prKey(a)}`, review: a, mine: true, reason: msg('In review') },
        1,
        a.review.updated_at,
      );
  }

  const started = new Set(
    input.work.flatMap(({ item }) => (item.ticket ? [`${item.ticket.account}:${item.ticket.key}`] : [])),
  );
  const todo = (t: TicketItem): boolean =>
    t.ticket.status.category !== 'done' && !waits(t, input.waiting) && !started.has(ticketKey(t));
  // Blocked tickets leave Next up, New for you and Up next; Next up keeps their place for when they clear.
  const open = (t: TicketItem): boolean => todo(t) && !t.ticket.blocked;
  const held = new Map<string, TicketItem>();
  for (const t of [...(input.nextUp ?? []), ...input.tickets.filter((t) => !input.parked?.has(ticketKey(t)))])
    if (todo(t) && t.ticket.blocked && !held.has(ticketKey(t))) held.set(ticketKey(t), t);
  // After the blocked work items (their ranks stop at 4), in Next up then tracker order.
  [...held.values()].forEach((t, i) =>
    add('blocked', { type: 'ticket', id: `t:${ticketKey(t)}`, ticket: t }, 9, String(i).padStart(6, '0')),
  );
  (input.nextUp ?? [])
    .filter(open)
    .forEach((t, i) =>
      add('next_up', { type: 'ticket', id: `n:${ticketKey(t)}`, ticket: t }, 0, String(i).padStart(6, '0')),
    );
  input.tickets
    .filter((t) => open(t) && !input.parked?.has(ticketKey(t)))
    .forEach((t, i) =>
      add(
        input.fresh?.has(ticketKey(t)) ? 'new' : 'up_next',
        { type: 'ticket', id: `t:${ticketKey(t)}`, ticket: t },
        t.ticket.status.category === 'in_progress' ? 0 : 1,
        String(i).padStart(6, '0'),
      ),
    );
  // After the work items (rank 9 is past every PHASE_ORDER), in Next up then tracker order.
  [...known.values()]
    .filter((t) => deploying(t) && !started.has(ticketKey(t)))
    .forEach((t, i) =>
      add('waiting', { type: 'ticket', id: `t:${ticketKey(t)}`, ticket: t }, 9, String(i).padStart(6, '0')),
    );

  // Oldest first, except In flight (latest activity first). Ticket sections keep their order (`at` = index).
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

/** A Now header chip: a count that scrolls to its section. */
export interface Chip {
  section: NowSection;
  n: number;
  text: string;
  /** The section's lamp when it needs Louis (Claude needs you, Fix); null stays muted. */
  lamp: Lamp | null;
}

/**
 * The Now header in section order, zero chips dropped:
 * `1 needs you`, `3 ready for review`, `2 to fix`, `2 review requests`, `1 Claude working`, `4 up next`.
 * Each chip reads like the section it jumps to.
 */
export function headerChips(sections: readonly Section[], working: number): Chip[] {
  const chip = (section: NowSection, n: number, text: string, lamp: Lamp | null = null): Chip[] =>
    n ? [{ section, n, text, lamp }] : [];
  const asks = count(sections, 'needs_you');
  const ready = count(sections, 'to_review');
  const fix = count(sections, 'fix');
  const prs = count(sections, 'requests');
  const next = count(sections, 'up_next');
  return [
    ...chip('needs_you', asks, tn('{n} needs you', '{n} need you', asks), 'needs_input'),
    ...chip('to_review', ready, t('{n} ready for review', { n: ready })),
    ...chip('fix', fix, tn('{n} to fix', '{n} to fix', fix), 'error'),
    ...chip('requests', prs, tn('{n} review request', '{n} review requests', prs)),
    ...chip('in_flight', working, t('{n} Claude working', { n: working })),
    ...chip('up_next', next, t('{n} up next', { n: next })),
  ];
}

/** The rail's Now line: the first chip that needs Louis, else the first one. */
export const topChip = (chips: readonly Chip[]): Chip | undefined => chips.find((c) => c.lamp) ?? chips[0];

// ---- Mod+J (ticket #136) ----------------------------------------------------------------------

/** Mod+J's bands, most urgent first. */
const BANDS = [
  msg('needs input'),
  msg('error'),
  msg('ready for review'),
  msg('feedback'),
  msg('review request'),
] as const;
/** Fix rows that are broken work rather than PR feedback. */
const BROKEN = new Set<PhaseId>(['failed', 'rebase_stopped']);

export interface Jump {
  row: NowRow;
  band: number;
  label: string;
}

/** When a row started waiting (oldest first within a band). */
function waitingSince(row: NowRow): string {
  switch (row.type) {
    case 'work':
      return row.item.claude_at ?? row.item.created_at;
    case 'session':
      return row.session.created_at;
    case 'outside':
      return row.session.updated_at;
    case 'review':
      return (row.mine ? null : row.review.review.requested_at) ?? row.review.review.updated_at;
    case 'ticket':
      return '';
  }
}

/** The row's short name in the HUD: `SHOP-142`, `#57`, the session name. */
export function rowKey(row: NowRow): string {
  switch (row.type) {
    case 'work':
      return workKey(row.item);
    case 'session':
      return row.session.name;
    case 'outside':
      return row.session.title;
    case 'review':
      return `#${row.review.review.ref.number}`;
    case 'ticket':
      return row.ticket.ticket.ref.key;
  }
}

/**
 * Mod+J's queue across projects: Claude needs input, error or rate-limited, ready for review, feedback
 * or red CI on my PRs, review requests (blocking first). Oldest first within a band.
 */
export function jumpQueue(sections: readonly Section[], sessions: readonly SessionInfo[]): Jump[] {
  const out: (Jump & { blocking: boolean; at: string })[] = [];
  const add = (row: NowRow, band: number, blocking = false): void => {
    out.push({ row, band, label: t(BANDS[band]!), blocking, at: waitingSince(row) });
  };
  for (const s of sections)
    for (const row of s.rows) {
      if (s.id === 'needs_you') add(row, 0);
      else if (s.id === 'to_review') add(row, 2);
      else if (s.id === 'fix') add(row, row.type === 'work' && BROKEN.has(row.phase.id) ? 1 : 3);
      else if (s.id === 'requests') add(row, 4, row.type === 'review' && row.review.review.blocking);
    }
  // A Claude stopped by an API error, rate limits included (StopFailure hook).
  for (const s of sessions)
    if (s.status === 'error' && s.lifecycle === 'live')
      add({ type: 'session', id: `e:${s.id}`, session: s }, 1);
  return out
    .sort((a, b) => a.band - b.band || Number(b.blocking) - Number(a.blocking) || a.at.localeCompare(b.at))
    .map(({ row, band, label }) => ({ row, band, label }));
}
