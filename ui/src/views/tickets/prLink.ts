// Ticket to PR join (UI only): the work item's `pr_url` first, else a loaded review whose
// `linked_tickets` names the ticket.

import type { CiState, Review, ReviewKind, Ticket } from '$lib/gen';
import { reviews } from '$lib/stores';
import type { LampLevel } from '$lib/stores/reducers';

import { sameKey } from '../reviews/linkedTicket';

const ALL = { kind: 'all' } as const;
const KINDS: ReviewKind[] = ['authored', 'review_requested'];

/** Every review of every loaded list (the PR candidates). */
export function loadedReviews(): Review[] {
  return Object.values(reviews.lists).flatMap((l) => l.data?.items.map((i) => i.review) ?? []);
}

/** Loads the review lists Now uses (all projects) once, so ticket rows can show their PR. */
export function ensureReviews(): void {
  for (const kind of KINDS) {
    const l = reviews.list(ALL, kind);
    if (!l.data && !l.loading && !l.error) void reviews.load(ALL, kind);
  }
}

/** `prUrl` = the ticket's work item PR. Keys match as review badges do (`#12` = `repo#12`). */
export function prForTicket(
  ticket: Ticket,
  prUrl: string | null,
  candidates: readonly Review[],
): Review | null {
  const byUrl = prUrl ? candidates.find((r) => r.url === prUrl) : undefined;
  if (byUrl) return byUrl;
  return candidates.find((r) => r.linked_tickets.some((k) => sameKey(ticket.ref.key, k, r.ref.repo))) ?? null;
}

/** `!42` for a GitLab MR, `#42` otherwise. */
export function prLabel(r: Review): string {
  return `${r.url.includes('/merge_requests/') ? '!' : '#'}${r.ref.number}`;
}

/** CI as a lamp shape: passed = dot, running = ring, failed = diamond. */
export function ciLamp(ci: CiState): LampLevel {
  return ci === 'success' ? 'done' : ci === 'pending' ? 'working' : ci === 'none' ? 'none' : 'error';
}

/** Review state word for the detail meta row. */
export function reviewWord(r: Review): string {
  if (r.draft) return 'Draft';
  if (r.decision === 'approved') return 'Approved';
  if (r.decision === 'changes_requested') return 'Changes requested';
  return 'Review required';
}
