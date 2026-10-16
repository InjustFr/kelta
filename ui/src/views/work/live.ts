// Store-connected view of a work item: its Claude session, PR, git status and phase.

import type { ProjectId, Review, SessionInfo, TicketRef, TrackerBinding, WorkItem } from '$lib/gen';
import { reviews, sessions, settings, ticketSteps, tickets, work } from '$lib/stores';
import { reviewKey } from '$lib/stores/reducers';
import { ticketKey } from '$lib/stores/tickets.svelte';

import { sessionTitle } from '../../shell/labels';
import { effectivePipeline } from '../settings/lib/pipeline';
import { phaseOf, questionOf, workKey, type Phase } from './phase';
import type { Stepped } from './stage';

const ALL = { kind: 'all' } as const;

export function unfinishedWork(): WorkItem[] {
  return work.all.filter((w) => w.state.kind !== 'finished');
}

/** The item's Claude session (live first). */
export function claudeOf(item: WorkItem): SessionInfo | null {
  const mine = item.session_ids
    .map((id) => sessions.get(id))
    .filter((s): s is SessionInfo => s?.kind.type === 'claude');
  return mine.find((s) => s.lifecycle === 'live') ?? mine[0] ?? null;
}

/** #502: Make this a work item is offered on a live Claude session in no unfinished work item. */
export function canMakeWorkItem(s: SessionInfo): boolean {
  if (s.kind.type !== 'claude' || s.lifecycle !== 'live') return false;
  return !unfinishedWork().some((w) => w.id === s.work_item_id || w.session_ids.includes(s.id));
}

/** The item's PR: by `pr_url`, else by branch (a PR Claude opened itself); review checkouts by ref. */
export function prOf(item: WorkItem): Review | null {
  if (item.review) {
    const r = item.review;
    // Now open and focus refresh it (`CodeHost::get`): fresher than the polled lists.
    const fresh = reviews.details[reviewKey(r)]?.data?.review;
    if (fresh) return fresh;
    const all = [...reviews.items(ALL, 'review_requested'), ...reviews.items(ALL, 'authored')];
    return (
      all.find(
        (it) =>
          it.review.ref.account === r.account &&
          it.review.ref.repo === r.repo &&
          it.review.ref.number === r.number,
      )?.review ?? null
    );
  }
  return (
    reviews
      .items(ALL, 'authored')
      .find(
        (it) =>
          (item.pr_url !== null && it.review.url === item.pr_url) ||
          (it.review.source_branch === item.branch && it.project_ids.includes(item.project_id)),
      )?.review ?? null
  );
}

/** The session asking a question for the item (#356): its run's session (a workflow), else its own. */
export function askerOf(item: WorkItem): SessionInfo | null {
  const ids = [item.run?.session_id, ...item.session_ids];
  return ids.map((id) => (id ? sessions.get(id) : null)).find((s) => questionOf(s)) ?? null;
}

export function phaseNow(item: WorkItem): Phase {
  return phaseOf(item, askerOf(item) ?? claudeOf(item), prOf(item), work.git[item.id] ?? null);
}

/** Title for rows and the palette: the ticket or PR title when a list has it, else the scratch task title or branch. */
export function workTitle(item: WorkItem): string {
  if (item.ticket) {
    const key = ticketKey(item.ticket);
    const detail = tickets.details[key]?.data?.ticket.title;
    if (detail) return detail;
    for (const l of Object.values(tickets.lists))
      for (const it of l.data?.items ?? []) if (ticketKey(it.ticket.ref) === key) return it.ticket.title;
    // Lists loaded but none has it: fetch it once (tracker_get, cached in details). Deferred: callers are reactive reads.
    const ref = item.ticket;
    const listed = Object.values(tickets.lists).some((l) => l.data);
    if (listed && !tickets.details[key])
      queueMicrotask(() => {
        if (!tickets.details[key]) void tickets.loadDetail(ref);
      });
  }
  return prOf(item)?.title ?? item.title ?? item.branch;
}

/** `KEY`, or `KEY #12` / `KEY feat/x-2` while the ticket has other unfinished items (#456). */
export function siblingKey(item: WorkItem): string {
  const key = workKey(item);
  const t = item.ticket;
  const others = work.all.some(
    (w) =>
      w.id !== item.id &&
      w.state.kind !== 'finished' &&
      w.ticket?.account === t?.account &&
      w.ticket?.key === t?.key,
  );
  if (!t || !others) return key;
  const n = prOf(item)?.ref.number ?? item.pr_url?.match(/(\d+)\/?$/)?.[1];
  return n ? `${key} #${n}` : `${key} ${item.branch}`;
}

/** `KEY claude`, `KEY nvim` for sessions of a work item, so three `claude` rows tell apart. */
export function sessionLabel(s: SessionInfo): string {
  const item = s.work_item_id ? work.get(s.work_item_id) : null;
  const name = sessionTitle(s);
  return item ? `${siblingKey(item)} ${name}` : name;
}

// No source found: the default steps, as `steps_of` (crates/kelta-work/src/pipeline.rs) with a default binding.
const NO_BINDING: Pick<TrackerBinding, 'status_map'> = {
  status_map: { start: null, review: null, done: null },
};

/** A ticket's stored step (#462) on its source's effective pipeline (the row's source, else `source`).
 *  Null when Kelta never moved the ticket or the step left the pipeline: callers then derive the stage. */
export function stepOf(
  ref: TicketRef | null,
  source: string | null = null,
  projectId: ProjectId | null = null,
): Stepped | null {
  const row = ref ? ticketSteps.get(ref) : null;
  const s = row ? settings.value(projectId) : null;
  if (!row || !s) return null;
  const src = s.sources.find((x) => x.id === (row.source_id ?? source));
  const steps = effectivePipeline(src?.tracker ?? NO_BINDING, s.work);
  const index = steps.findIndex((x) => x.id === row.step_id);
  return index < 0 ? null : { index, steps: steps.map((x) => x.label) };
}

export const stepNow = (w: WorkItem): Stepped | null =>
  stepOf(w.ticket, w.ticket_source ?? null, w.project_id);
