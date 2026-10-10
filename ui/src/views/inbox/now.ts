// Now, fed from the stores: sections, header, freshness, row actions and Next waiting (FLOW §3).
// Used by the Now pane, the rail tile (badge + tooltip) and `attention.next`.

import type { KeltaError } from '$lib/gen';
import { projects, reviews, sessions, tickets, work } from '$lib/stores';

import { hud } from '../../shell/hud.svelte';
import { flushJump } from '../../shell/jumplist';
import { revealSession } from '../../shell/nav';
import { runPrimary } from '../work/actions';
import { phaseNow, unfinishedWork } from '../work/live';
import { goToWork, openFromNow, openReview } from '../work/nav';
import { reviewLocally, startWorkOnTicket } from '../work/startWork';
import {
  headerParts,
  jumpQueue,
  nowSections,
  rowKey,
  waitingCount,
  type NowRow,
  type Section,
} from './groups';

const ALL = { kind: 'all' } as const;

export function currentSections(): Section[] {
  return nowSections({
    work: unfinishedWork().map((item) => ({ item, phase: phaseNow(item) })),
    sessions: sessions.all,
    requested: reviews.items(ALL, 'review_requested'),
    authored: reviews.items(ALL, 'authored'),
    tickets: tickets.items(ALL, null),
  });
}

/** Claude sessions in Working: the parallel capacity in use. */
export function workingCount(): number {
  return sessions.all.filter((s) => s.kind.type === 'claude' && s.status === 'working').length;
}

export interface NowSummary {
  sections: Section[];
  parts: string[];
  /** The header as one line (rail tooltip). */
  header: string;
  waiting: number;
}

export function nowSummary(): NowSummary {
  const sections = currentSections();
  const parts = headerParts(sections, workingCount());
  return {
    sections,
    parts,
    header: parts.join('\u2002\u2002') || 'Nothing waiting',
    waiting: waitingCount(sections),
  };
}

/** "as of hh:mm" when a source is stale or failed; null when everything is fresh. */
export function asOf(): { at: string; ms: number; error: KeltaError | null } | null {
  const lists = [
    tickets.list(ALL, null),
    reviews.list(ALL, 'review_requested'),
    reviews.list(ALL, 'authored'),
  ];
  const bad = lists.filter((l) => l.stale || l.error || (l.data?.errors.length ?? 0) > 0);
  if (bad.length === 0 && !work.gitError) return null;
  const times = [...bad.map((l) => l.fetchedAt), work.gitError ? work.gitAt : null].filter(
    (t): t is number => t !== null,
  );
  const ms = times.length ? Math.min(...times) : Date.now();
  const d = new Date(ms);
  const at = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  return { at, ms, error: bad.find((l) => l.error)?.error ?? null };
}

/**
 * Startup, window focus and Now open (no polling): cached lists unless `force`, fresh git status,
 * and my review state + head of review items (FLOW §3.6).
 */
export async function refreshNow(force = false): Promise<void> {
  await Promise.allSettled([
    tickets.load(ALL, null, force),
    reviews.load(ALL, 'review_requested', force),
    reviews.load(ALL, 'authored', force),
    work.refreshStatus(),
    ...unfinishedWork().flatMap((w) => (w.review ? [reviews.loadDetail(w.review)] : [])),
  ]);
}

function rowProject(ids: readonly string[]): string | null {
  return ids[0] ?? projects.activeId;
}

/** `g`: the row's place, without acting. Leaves Now. */
export async function goToRow(row: NowRow): Promise<void> {
  switch (row.type) {
    case 'work':
      await goToWork(row.item);
      return;
    case 'session':
      await revealSession(row.session.id);
      return;
    case 'review': {
      const pid = rowProject(row.review.project_ids);
      if (pid) await openReview(pid, { kind: 'review_detail', review: row.review.review.ref });
      return;
    }
    case 'ticket': {
      const pid = rowProject(row.ticket.project_ids);
      if (pid) await openFromNow(pid, { kind: 'ticket_detail', ticket: row.ticket.ticket.ref });
    }
  }
}

/** `Enter`: the row's next action. */
export async function enterRow(row: NowRow): Promise<void> {
  if (row.type === 'work') await runPrimary(row.item);
  else if (row.type === 'ticket')
    await startWorkOnTicket(row.ticket.ticket.ref, rowProject(row.ticket.project_ids));
  // shortcut: a PR of mine without a work item opens its review until Fix with Claude adopts it.
  else await goToRow(row);
}

/** `s` on a Review requests row. */
export function reviewRowLocally(row: Extract<NowRow, { type: 'review' }>): void {
  void reviewLocally(row.review.review.ref, rowProject(row.review.project_ids));
}

let lastJump: string | null = null;

/**
 * `attention.next` / `attention.prev` (Mod+J / Mod+Shift+J): goes to the next or previous item of the
 * jump queue, cycling, and shows `2/7 · needs input · SHOP-142`. With nothing waiting, `Enter` on the
 * HUD opens Up next (in Now), or Tickets when Up next is empty.
 */
export async function nextWaiting(delta: 1 | -1 = 1): Promise<void> {
  flushJump();
  const sections = currentSections();
  const queue = jumpQueue(sections, sessions.all);
  if (queue.length === 0) {
    // shortcut: "queued · parked" and the Next up view wait on #145; Up next stands in for them.
    const up = sections.find((s) => s.id === 'up_next');
    const queued = up ? up.rows.length + up.more : 0;
    if (queued) hud.show(`nothing waiting · ${queued} up next`, { action: 'inbox.open', label: 'Up next' });
    else hud.show('nothing waiting', { action: 'tickets.open', label: 'Tickets' });
    return;
  }
  const at = queue.findIndex((j) => j.row.id === lastJump);
  const i = at < 0 ? (delta > 0 ? 0 : queue.length - 1) : (at + delta + queue.length) % queue.length;
  const jump = queue[i]!;
  lastJump = jump.row.id;
  hud.show(`${i + 1}/${queue.length} · ${jump.label} · ${rowKey(jump.row)}`);
  await goToRow(jump.row);
}
