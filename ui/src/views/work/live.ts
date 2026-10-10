// Store-connected view of a work item: its Claude session, PR, git status and phase.

import type { Review, SessionInfo, WorkItem } from '$lib/gen';
import { reviews, sessions, tickets, work } from '$lib/stores';
import { ticketKey } from '$lib/stores/tickets.svelte';

import { phaseOf, workKey, type Phase } from './phase';

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

/** The item's PR: by `pr_url`, else by branch (a PR Claude opened itself); review checkouts by ref. */
export function prOf(item: WorkItem): Review | null {
  if (item.review) {
    const r = item.review;
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

export function phaseNow(item: WorkItem): Phase {
  return phaseOf(item, claudeOf(item), prOf(item), work.git[item.id] ?? null);
}

/** Title for rows and the palette: the ticket or PR title when a list has it, else the scratch task title or branch. */
export function workTitle(item: WorkItem): string {
  if (item.ticket) {
    const key = ticketKey(item.ticket);
    const detail = tickets.details[key]?.data?.ticket.title;
    if (detail) return detail;
    for (const l of Object.values(tickets.lists))
      for (const it of l.data?.items ?? []) if (ticketKey(it.ticket.ref) === key) return it.ticket.title;
  }
  return prOf(item)?.title ?? item.title ?? item.branch;
}

/** `KEY claude`, `KEY nvim` for sessions of a work item, so three `claude` rows tell apart. */
export function sessionLabel(s: SessionInfo): string {
  const item = s.work_item_id ? work.get(s.work_item_id) : null;
  return item ? `${workKey(item)} ${s.name}` : s.name;
}
