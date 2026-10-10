// My PRs: who a PR waits on, for how long, and the nudge cooldown (#209).
import type { Review } from '$lib/gen';

const HOUR = 3600_000;
/** Mirrors core's `NUDGE_COOLDOWN` (core refuses a nudge within it). */
export const NUDGE_COOLDOWN_MS = 24 * HOUR;

const mentions = (r: Review): string[] => r.waiting_on.map((w) => `@${w}`);

/** `waiting on @anna · 26h`, `late` past `slaHours`; null when the PR waits on nobody. */
export function waitingChip(
  r: Review,
  slaHours: number,
  now = Date.now(),
): { label: string; late: boolean } | null {
  if (r.waiting_on.length === 0) return null;
  const waited = Math.max(0, now - Date.parse(r.requested_at ?? r.updated_at));
  return {
    label: `waiting on ${mentions(r).join(', ')} · ${Math.floor(waited / HOUR)}h`,
    late: waited > slaHours * HOUR,
  };
}

export function nudgedRecently(r: Review, now = Date.now()): boolean {
  return !!r.nudged_at && now - Date.parse(r.nudged_at) < NUDGE_COOLDOWN_MS;
}

/** `reviews.nudge_template` with `{reviewers}` = `@anna @bob`. */
export function pingBody(template: string, r: Review): string {
  return template.replaceAll('{reviewers}', mentions(r).join(' '));
}
