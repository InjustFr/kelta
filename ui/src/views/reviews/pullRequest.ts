// The Story panel's Pull request section (#426): checks failed-first, reviewer states and who the PR waits on.
import type { CiCheck, Review } from '$lib/gen';
import { msg } from '$lib/i18n/index.svelte';
import { reviews } from '$lib/stores';
import { reviewKey } from '$lib/stores/reducers';

import type { WaitChip } from '../people/waiting';
import { waitingChip } from './nudge';

const ALL = { kind: 'all' } as const;

/** A reviewer's latest state in words (keys; translate with `t`). */
export const REVIEWER_STATE: Record<string, string> = {
  approved: msg('approved'),
  changes_requested: msg('changes requested'),
  commented: msg('commented'),
  pending: msg('pending'),
  dismissed: msg('dismissed'),
};

const rank = (c: CiCheck): number => (c.state === 'failure' || c.state === 'error' ? 0 : c.state === 'pending' ? 1 : 2);

/** Failed checks first, then running, then the rest; each group keeps the host's order. */
export const checksFirst = (checks: readonly CiCheck[]): CiCheck[] => [...checks].sort((a, b) => rank(a) - rank(b));

/** `waiting on Anna · 26h` from the polled lists (null when the PR is in none of them yet). */
export function prWait(rv: Review, slaHours: number): WaitChip | null {
  const key = reviewKey(rv.ref);
  const item = [...reviews.items(ALL, 'authored'), ...reviews.items(ALL, 'review_requested')].find(
    (i) => reviewKey(i.review.ref) === key,
  );
  return item ? waitingChip(item, slaHours) : null;
}
