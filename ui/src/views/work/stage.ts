// The stage track (#399): where an item stands on the way from ticket to deploy, and whose move it is.
// Pure; Now draws it on every row, Tickets and Reviews reuse it (#400, #401).

import { msg, t } from '$lib/i18n/index.svelte';
import type { Review, WorkItem } from '$lib/gen';

import { ZONES, zoneOf } from '../inbox/groups';
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

/** Whose move: filled for mine, a ring while it waits on others, a diamond when something broke, a dot once done. */
export type Mark = 'you' | 'wait' | 'broken' | 'done';

export interface Stage {
  /** Where the item is now (an index of STAGES). */
  at: number;
  /** The furthest stage it reached: an item sent back to Claude after review keeps the line to Review. */
  reached: number;
  mark: Mark;
  /** Only the tracker's word for it (#400): no branch or PR seen, so the line is dotted. */
  unconfirmed?: boolean;
  /** A pipeline track (#462): Ticket, then the source's step labels; STAGES when absent. */
  labels?: readonly string[];
  /** Claude works on the item: a working lamp on the current step (#462). */
  working?: boolean;
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

/** A ticket's stored pipeline step (#462): its index in `steps`, the labels of its source's effective pipeline. */
export interface Stepped {
  index: number;
  steps: readonly string[];
}

/** `s` on the stored step: the track is Ticket then the steps, the step is where the ticket stands (forward
 *  only, so also the furthest), and `s` (the phase's stage) keeps only its mark. */
export function onStep(s: Stage, step: Stepped, working = false): Stage {
  const at = step.index + 1;
  return {
    at,
    reached: at,
    mark: s.mark,
    labels: [STAGES[TICKET], ...step.steps],
    ...(working ? { working } : {}),
  };
}

/** The name of the stage the item is at, translated. */
export const stageName = (s: Stage): string => t((s.labels ?? STAGES)[s.at]!);

/** A column header's last stage (#481): where every row's track ends (`Stage.labels`, else STAGES), blank when they differ. */
export function trackEnd(tracks: readonly (readonly string[] | null | undefined)[]): string {
  const ends = new Set(tracks.map((l) => (l ?? STAGES)[(l ?? STAGES).length - 1]!));
  return ends.size === 1 ? t([...ends][0]!) : '';
}

/** A work item's stage. `mine`: its row is in Your move. `deploying`: its ticket waits for a deploy.
 *  `step`: the ticket's stored step (#462), which places it; the phase then decides only the mark. */
export function workStage(
  item: Pick<WorkItem, 'pr_url' | 'review' | 'feedback_at'>,
  phase: Pick<Phase, 'id' | 'lamp'>,
  pr: Pick<Review, 'decision'> | null,
  mine: boolean,
  deploying = false,
  step: Stepped | null = null,
): Stage {
  const reviewed = !!item.feedback_at || pr?.decision === 'changes_requested' || pr?.decision === 'approved';
  const opened = !!item.pr_url || !!item.review || pr !== null;
  const s = deploying
    ? stage(DEPLOY, 'wait')
    : stage(
        AT[phase.id] ?? CLAUDE,
        phase.lamp === 'error' ? 'broken' : mine ? 'you' : 'wait',
        reviewed ? REVIEW : opened ? PR : CLAUDE,
      );
  return step ? onStep(s, step, phase.lamp === 'working') : s;
}

/** The work tab header (#428): the item's stage, its mark by Now's zone, and why in one sentence. */
export function workHeader(
  item: Pick<WorkItem, 'pr_url' | 'review' | 'feedback_at'>,
  phase: Pick<Phase, 'id' | 'lamp' | 'label' | 'detail' | 'section'>,
  pr: Pick<Review, 'decision'> | null,
  step: Stepped | null = null,
): { stage: Stage; why: string } {
  const zone = zoneOf(phase.section);
  return {
    stage: workStage(item, phase, pr, zone === 'yours', false, step),
    why: t('{whose}: {what}', {
      whose: t(ZONES.find((z) => z.id === zone)!.label),
      what: phase.detail ? `${phase.label}, ${phase.detail}` : phase.label,
    }),
  };
}
