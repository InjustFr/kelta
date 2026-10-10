// Ticket to PR link (TICKETS.md T1): the core fills `TicketItem.prs` / `TicketDetail.prs` (work item
// PR first, then PRs whose `linked_tickets` name the ticket); this module labels and opens them.

import type { CiState, PrLink, ProjectId, ReviewKind } from '$lib/gen';
import { openExternal } from '$lib/ipc/commands';
import { reviews, toasts } from '$lib/stores';
import type { LampLevel } from '$lib/stores/reducers';
import type { MenuItem } from '$lib/ui';

import { openContent } from '../work/nav';

const ALL = { kind: 'all' } as const;
const KINDS: ReviewKind[] = ['authored', 'review_requested'];

/** Loads the review lists once so the core's review cache (the key-match source) is warm. */
export function ensureReviews(): void {
  for (const kind of KINDS) {
    const l = reviews.list(ALL, kind);
    if (!l.data && !l.loading && !l.error) void reviews.load(ALL, kind);
  }
}

/** The PR a chip shows: the first open one, else the first (merged/closed). */
export function mainPr(prs: readonly PrLink[]): PrLink | null {
  return prs.find((p) => p.state === 'open') ?? prs[0] ?? null;
}

/** `!42` for a GitLab MR, `#42` otherwise. */
export function prLabel(pr: PrLink): string {
  return `${pr.url.includes('/merge_requests/') ? '!' : '#'}${pr.number}`;
}

/** CI as a lamp shape: passed = dot, running = ring, failed = diamond. */
export function ciLamp(ci: CiState): LampLevel {
  return ci === 'success' ? 'done' : ci === 'pending' ? 'working' : ci === 'none' ? 'none' : 'error';
}

/** One lamp for CI and review (DESIGN §6.1 precedence): changes requested = needs input, then CI failed or
 * running, approved = done, awaiting a required review = activity, else the CI shape. */
export function prLamp(pr: PrLink): LampLevel {
  if (pr.review === 'changes_requested') return 'needs_input';
  const ci = ciLamp(pr.ci);
  if (ci === 'error' || ci === 'working' || pr.review === null) return ci;
  return pr.review === 'approved' ? 'done' : 'activity';
}

/** Review state word for the detail meta row. */
export function reviewWord(pr: PrLink): string {
  if (pr.state === 'merged') return 'Merged';
  if (pr.state === 'closed') return 'Closed';
  if (pr.draft) return 'Draft';
  if (pr.review === 'approved') return 'Approved';
  if (pr.review === 'changes_requested') return 'Changes requested';
  return 'Review required';
}

/** `p`: Kelta's review detail when the code host is bound; `P` (browser) or unbound: the browser. */
export function openPr(pr: PrLink, projectId: ProjectId, browser = false): void {
  if (!browser && pr.account) {
    void openContent(projectId, {
      kind: 'review_detail',
      review: { account: pr.account, repo: pr.repo, number: pr.number },
    });
    return;
  }
  openExternal({ url: pr.url }).catch((err) => toasts.error(err, 'Open in browser'));
}

/** PR picker rows (several PRs on one ticket). */
export function prMenuItems(prs: readonly PrLink[]): MenuItem[] {
  return prs.map((p) => ({ id: p.url, label: `${prLabel(p)} ${p.title || p.repo}` }));
}

/** Opens the only PR, or returns true when the caller should show the picker. */
export function openPrs(
  key: string,
  prs: readonly PrLink[],
  projectId: ProjectId,
  browser: boolean,
): boolean {
  const [only] = prs;
  if (!only) toasts.info(`No PR linked to ${key}`);
  else if (prs.length === 1) openPr(only, projectId, browser);
  return prs.length > 1;
}
