// My PRs: who a PR waits on, for how long, and the nudge cooldown (#209).
import type { Review, ReviewItem } from '$lib/gen';

import { waitChip, type WaitChip } from '../people/waiting';

const HOUR = 3600_000;
/** Mirrors core's `NUDGE_COOLDOWN` (core refuses a nudge within it). */
export const NUDGE_COOLDOWN_MS = 24 * HOUR;

const mentions = (r: Review): string[] => r.waiting_on.map((w) => `@${w}`);

/** The row's wait chip; amber past `slaHours` on my PRs only (a request row's wait is not mine to chase). */
export function waitingChip(item: ReviewItem, slaHours: number, now = Date.now()): WaitChip | null {
  const mine = item.review.kind === 'authored';
  return waitChip(item.waiting, {
    slaHours: mine ? slaHours : undefined,
    fallbackSince: mine ? item.review.updated_at : undefined,
    now,
  });
}

export function nudgedRecently(r: Review, now = Date.now()): boolean {
  return !!r.nudged_at && now - Date.parse(r.nudged_at) < NUDGE_COOLDOWN_MS;
}

/** `reviews.nudge_template` with `{reviewers}` = `@anna @bob`. */
export function pingBody(template: string, r: Review): string {
  return template.replaceAll('{reviewers}', mentions(r).join(' '));
}
