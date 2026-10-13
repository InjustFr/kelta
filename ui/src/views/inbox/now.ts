// Now, fed from the stores: sections, header, freshness, row actions and Next waiting (FLOW §3).
// Used by the Now pane, the rail tile (badge + tooltip) and `attention.next`.

import { formatDate, t } from '$lib/i18n/index.svelte';
import type { KeltaError, TicketRef } from '$lib/gen';
import { outsideJump } from '$lib/ipc/commands';
import { projects, reviews, sessions, tickets, toasts, ui, work } from '$lib/stores';
import { ticketKey } from '$lib/stores/tickets.svelte';

import { hud } from '../../shell/hud.svelte';
import { flushJump } from '../../shell/jumplist';
import { revealSession } from '../../shell/nav';
import { peek } from '../../shell/peek.svelte';
import { runPrimary } from '../work/actions';
import { batch, claudeSlots, startsNow } from '../work/batch.svelte';
import { phaseNow, unfinishedWork } from '../work/live';
import { goToWork, openFromNow, openReview } from '../work/nav';
import { reviewLocally, startWorkOnTicket } from '../work/startWork';
import { jumpQueue, nowSections, rowKey, waitingCount, zoneCount, type NowRow, type Section } from './groups';
import { loadPool, nextUp, ticketPool, waitingStatuses } from './nextUp.svelte';

const ALL = { kind: 'all' } as const;
/** Up next is my tickets across every source, whatever each view's own `who` says. */
const MINE = 'mine' as const;

export function currentSections(): Section[] {
  const mine = tickets.items(ALL, null, MINE);
  const pool = new Map(ticketPool().map((t) => [ticketKey(t.ticket.ref), t]));
  const now = Date.now();
  const keys = (pick: (ref: TicketRef) => boolean): Set<string> =>
    new Set(mine.filter((t) => pick(t.ticket.ref)).map((t) => ticketKey(t.ticket.ref)));
  return nowSections({
    work: unfinishedWork().map((item) => ({ item, phase: phaseNow(item) })),
    sessions: sessions.all,
    outside: sessions.elsewhere,
    requested: reviews.items(ALL, 'review_requested'),
    authored: reviews.items(ALL, 'authored'),
    tickets: mine,
    // shortcut: a Next up ticket neither list has (a later page) is not shown until it loads.
    nextUp: nextUp.ordered(now).flatMap((e) => pool.get(ticketKey(e.ticket)) ?? []),
    parked: keys((ref) => nextUp.parked(ref, now)),
    fresh: keys((ref) => nextUp.isNew(ref)),
    waiting: waitingStatuses(),
  });
}

export interface NowSummary {
  sections: Section[];
  /** Rows in Your move and in Waiting on others (#399). */
  yours: number;
  others: number;
  /** The rail's Now line. */
  top: string;
  /** The rail badge: what waits on Louis now (Claude asking, diffs, fixes, review requests). */
  waiting: number;
}

export function nowSummary(): NowSummary {
  const sections = currentSections();
  const yours = zoneCount(sections, 'yours');
  const others = zoneCount(sections, 'waiting');
  return {
    sections,
    yours,
    others,
    top: yours
      ? t('{n} your move', { n: yours })
      : others
        ? t('{n} waiting on others', { n: others })
        : t('Nothing waiting'),
    waiting: waitingCount(sections),
  };
}

/** "as of hh:mm" when a source is stale or failed; null when everything is fresh. */
export function asOf(): { at: string; ms: number; error: KeltaError | null } | null {
  const lists = [
    tickets.list(ALL, null, MINE),
    reviews.list(ALL, 'review_requested'),
    reviews.list(ALL, 'authored'),
  ];
  const bad = lists.filter((l) => l.stale || l.error || (l.data?.errors.length ?? 0) > 0);
  if (bad.length === 0 && !work.gitError) return null;
  const times = [...bad.map((l) => l.fetchedAt), work.gitError ? work.gitAt : null].filter(
    (t): t is number => t !== null,
  );
  const ms = times.length ? Math.min(...times) : Date.now();
  const at = formatDate(ms, { hour: '2-digit', minute: '2-digit' });
  return { at, ms, error: bad.find((l) => l.error)?.error ?? null };
}

/**
 * Startup, window focus and Now open (no polling): cached lists unless `force`, fresh git status,
 * and my review state + head of review items (FLOW §3.6).
 */
export async function refreshNow(force = false): Promise<void> {
  await Promise.allSettled([
    loadPool(force),
    reviews.load(ALL, 'review_requested', force),
    reviews.load(ALL, 'authored', force),
    work.refreshStatus(),
    sessions.loadOutside(),
    ...unfinishedWork().flatMap((w) => (w.review ? [reviews.loadDetail(w.review)] : [])),
  ]);
  await nextUp.prune();
}

/** `S` on Next up: its top N through batch start (#141), N = the free Claude slots (all when uncapped). */
export function startNextUp(rows: readonly NowRow[]): void {
  const { live, max } = claudeSlots();
  const n = startsNow(rows.length, live, max);
  if (n === 0) {
    toasts.info(t('No free Claude slot ({live}/{max} live)', { live, max }));
    return;
  }
  batch.clear();
  for (const r of rows.slice(0, n))
    if (r.type === 'ticket') batch.toggle(r.ticket.ticket.ref, r.ticket.project_ids[0] ?? null);
  ui.openSheet('start_batch');
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
    case 'outside':
      await jumpOutside(row.session.id);
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

/** A Claude session Kelta did not start: its pane and window, else Peek with where it runs (#346). */
export async function jumpOutside(id: string): Promise<void> {
  try {
    const place = await outsideJump({ id });
    if (place !== null) peek.openOutside(id, place);
  } catch (err) {
    toasts.error(err, t('Could not reach that Claude session'));
  }
}

/** `Enter`: the row's next action. */
export async function enterRow(row: NowRow): Promise<void> {
  if (row.type === 'work') await runPrimary(row.item);
  else if (row.type === 'ticket')
    await startWorkOnTicket(row.ticket.ticket.ref, null, { source: row.ticket.source_ids[0] ?? null });
  // shortcut: a PR of mine without a work item opens its review until Fix with Claude adopts it.
  else await goToRow(row);
}

/** `s` on a Review requests row. */
export function reviewRowLocally(row: Extract<NowRow, { type: 'review' }>): void {
  void reviewLocally(row.review.review.ref, row.review.project_ids[0]);
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
    const up = sections.find((s) => s.id === 'next_up') ?? sections.find((s) => s.id === 'up_next');
    const queued = up ? up.rows.length + up.more : 0;
    if (up && queued)
      hud.show(`${t('nothing waiting')} · ${queued} ${t(up.label).toLowerCase()}`, {
        action: 'inbox.open',
        label: t(up.label),
      });
    else hud.show(t('nothing waiting'), { action: 'tickets.open', label: t('Tickets') });
    return;
  }
  const at = queue.findIndex((j) => j.row.id === lastJump);
  const i = at < 0 ? (delta > 0 ? 0 : queue.length - 1) : (at + delta + queue.length) % queue.length;
  const jump = queue[i]!;
  lastJump = jump.row.id;
  hud.show(`${i + 1}/${queue.length} · ${jump.label} · ${rowKey(jump.row)}`);
  await goToRow(jump.row);
}
