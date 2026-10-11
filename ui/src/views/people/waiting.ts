// "Waiting on X" chips (#240): the core's `Waiting[]` as a label, a hover and a lateness flag.
// One formatter for Tickets, Reviews and Now.

import { msg, t } from '$lib/i18n/index.svelte';
import type { Waiting } from '$lib/gen';

const HOUR = 3600_000;

export interface WaitChip {
  /** `waiting on Anna · 26h`, `waiting on Anna +2`, `waiting on Bob (author)`. */
  label: string;
  /** One line per person with the reason. */
  title: string;
  /** Waited past `slaHours` (only when one is given). */
  late: boolean;
}

const hours = (since: string | number, now: number): number =>
  Math.floor(Math.max(0, now - (typeof since === 'string' ? Date.parse(since) : since)) / HOUR);
const who = (w: Waiting): string => (w.person === 'me' ? t('you') : w.name);
const WHY: Record<Waiting['why'], string> = {
  author: msg('author'),
  assigned: msg('assigned'),
  review: msg('review'),
};

function reason(w: Waiting, now: number): string {
  if (w.why === 'author') return t('changes requested, author to act');
  if (w.why === 'assigned') return t('assigned');
  return w.since ? t('review requested {n}h ago', { n: hours(w.since, now) }) : t('review requested');
}

/**
 * `waits` is the core's list, `null` when nobody is waited on. `since` falls back to `fallbackSince` for
 * waits without a time. `withWhy` names the reason in the label (Now); an author wait always does.
 */
export function waitChip(
  waits: readonly Waiting[],
  opts: { slaHours?: number; fallbackSince?: string; withWhy?: boolean; now?: number } = {},
): WaitChip | null {
  const [first, ...rest] = waits;
  if (!first) return null;
  const now = opts.now ?? Date.now();
  // The fallback and lateness belong to review waits: an author wait is not a slow reviewer.
  const oldestOf = (ws: readonly Waiting[], fallback?: string): number | null => {
    const ts = ws.map((w) => w.since ?? fallback).filter((s): s is string => !!s);
    return ts.length > 0 ? Math.min(...ts.map((s) => Date.parse(s))) : null;
  };
  const reviews = waits.filter((w) => w.why === 'review');
  const oldest = oldestOf(waits.filter((w) => w.why !== 'review'));
  const oldestReview = oldestOf(reviews, opts.fallbackSince);
  const oldestAll = [oldest, oldestReview].filter((t): t is number => t !== null);
  const aged = oldestAll.length > 0 ? Math.min(...oldestAll) : null;
  const why = opts.withWhy || first.why === 'author' ? ` (${t(WHY[first.why])})` : '';
  const age = rest.length === 0 && aged !== null ? ` · ${hours(aged, now)}h` : '';
  const more = rest.length > 0 ? ` +${rest.length}` : '';
  const title = waits
    .map((w) => {
      const note = w.matched
        ? ''
        : `. ${w.why === 'assigned' ? t('Not matched to a code host user') : t('Not matched to a tracker user')}`;
      return `${who(w)}: ${reason(w, now)}${note}`;
    })
    .join('\n');
  return {
    label: `${t('waiting on {who}', { who: who(first) })}${why}${more}${age}`,
    title,
    late: opts.slaHours !== undefined && oldestReview !== null && now - oldestReview > opts.slaHours * HOUR,
  };
}
