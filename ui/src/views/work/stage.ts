// The stage track (#399): where an item stands on the way from ticket to deploy, and whose move it is.
// Pure; Now draws it on every row, Tickets and Reviews reuse it (#400, #401).

import { msg } from '$lib/i18n/index.svelte';
import type { Review, WorkItem } from '$lib/gen';

import type { Phase, PhaseId } from './phase';

export const STAGES = [
  msg('Ticket'),
  msg('Claude'),
  msg('PR'),
  msg('Review'),
  msg('Merge'),
  msg('Deploy'),
] as const;

export const TICKET = 0;
export const CLAUDE = 1;
export const PR = 2;
export const REVIEW = 3;
export const MERGE = 4;
export const DEPLOY = 5;

/** Whose move: filled for mine, a ring while it waits on others, a diamond when something broke. */
export type Mark = 'you' | 'wait' | 'broken';

export interface Stage {
  /** Where the item is now (an index of STAGES). */
  at: number;
  /** The furthest stage it reached: an item sent back to Claude after review keeps the line to Review. */
  reached: number;
  mark: Mark;
}

/** Phases past Claude; every other phase sits with Claude (asking, working, to review, failed start). */
const AT: Partial<Record<PhaseId, number>> = {
  unpushed: PR,
  ready: PR,
  rebased: PR,
  remote_new: PR,
  draft: PR,
  checks_failed: PR,
  in_review: REVIEW,
  changes_requested: REVIEW,
  new_comments: REVIEW,
  reviewing: REVIEW,
  updated: REVIEW,
  reviewed: REVIEW,
  approved: MERGE,
  conflicts: MERGE,
  merged: MERGE,
  closed: MERGE,
};

export const stage = (at: number, mark: Mark, reached = at): Stage => ({
  at,
  reached: Math.max(at, reached),
  mark,
});

/** A work item's stage. `mine`: its row is in Your move. `deploying`: its ticket waits for a deploy. */
export function workStage(
  item: Pick<WorkItem, 'pr_url' | 'review' | 'feedback_at'>,
  phase: Pick<Phase, 'id' | 'lamp'>,
  pr: Pick<Review, 'decision'> | null,
  mine: boolean,
  deploying = false,
): Stage {
  if (deploying) return stage(DEPLOY, 'wait');
  const reviewed = !!item.feedback_at || pr?.decision === 'changes_requested' || pr?.decision === 'approved';
  const opened = !!item.pr_url || !!item.review || pr !== null;
  return stage(
    AT[phase.id] ?? CLAUDE,
    phase.lamp === 'error' ? 'broken' : mine ? 'you' : 'wait',
    reviewed ? REVIEW : opened ? PR : CLAUDE,
  );
}
