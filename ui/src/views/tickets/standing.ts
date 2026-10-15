// Where a ticket stands (#400): its place on Now's stage track (#399) and one line on the situation,
// from its live work item when there is one, else from its linked PR and its tracker status.

import { t } from '$lib/i18n/index.svelte';
import type { Review, TicketItem, WorkItem } from '$lib/gen';
import { sessions, tickets, work } from '$lib/stores';
import { ticketKey } from '$lib/stores/tickets.svelte';

import { waits, zoneOf } from '../inbox/groups';
import { ticketWork } from '../work/startWork';
import { waitingStatuses } from '../inbox/nextUp.svelte';
import { phaseNow, prOf, stepOf } from '../work/live';
import { outsidePhase, type Phase } from '../work/phase';
import {
  CLAUDE,
  DEPLOY,
  MERGE,
  PR,
  REVIEW,
  TICKET,
  onStep,
  stage,
  workStage,
  type Stage,
  type Stepped,
} from '../work/stage';
import { mainPr, prLabel } from './prs';

export interface Standing {
  stage: Stage;
  /** The situation in a sentence. */
  where: string;
}

/**
 * `work`: the ticket's unfinished work item, its phase and PR (as Now reads them). `mine`: assigned to me.
 * `waiting`: the ticket sits in a status its source lists as waiting for a deploy (#391).
 * `step`: the ticket's stored pipeline step (#462); without it, the work item, PR and tracker place the ticket as before.
 */
export function standing(
  item: Pick<TicketItem, 'ticket' | 'prs'>,
  work: { item: WorkItem; phase: Phase; pr: Review | null } | null,
  mine: boolean,
  waiting: boolean,
  step: Stepped | null = null,
): Standing {
  const deploy = t('Waiting for a deploy. Nothing for you to do.');
  if (item.ticket.status.category === 'done') return { stage: stage(DEPLOY, 'done'), where: t('Done.') };
  if (work) {
    const { phase } = work;
    // As in Now: Claude needs you and Fix keep their place even in a waiting status.
    const deploying = waiting && phase.section !== 'needs_you' && phase.section !== 'fix';
    const yours = zoneOf(phase.section) === 'yours';
    return {
      stage: workStage(work.item, phase, work.pr, yours, deploying, step),
      where: deploying
        ? deploy
        : yours
          ? t('{phase}. Your move in the Queue.', { phase: phase.label })
          : t('{phase}.', { phase: phase.label }),
    };
  }
  const r = byPr(item, mine, waiting, deploy);
  return step ? { ...r, stage: onStep(r.stage, step) } : r;
}

/** Without a work item: the linked PR, else the tracker's word for it (#400). */
function byPr(item: Pick<TicketItem, 'prs'>, mine: boolean, waiting: boolean, deploy: string): Standing {
  const pr = mainPr(item.prs);
  const live = pr && pr.state !== 'closed' ? pr : null;
  // No branch or PR seen: the track takes the tracker's word for the stages before.
  if (waiting) return { stage: { ...stage(DEPLOY, 'wait'), unconfirmed: !live }, where: deploy };
  const mark = mine ? 'you' : 'wait';
  if (!live) return { stage: stage(TICKET, mark), where: t('Not started in Kelta.') };
  if (live.state === 'merged') return { stage: stage(MERGE, 'wait'), where: t('The PR is merged.') };
  if (live.ci === 'failure' || live.ci === 'error')
    return {
      stage: stage(PR, 'broken', live.review ? REVIEW : PR),
      where: t('A check failed on the PR.'),
    };
  if (live.draft) return { stage: stage(PR, mark), where: t('The PR is still a draft.') };
  if (live.review === 'approved') return { stage: stage(MERGE, mark), where: t('Approved, ready to merge.') };
  if (live.review === 'changes_requested')
    return { stage: stage(CLAUDE, mark, REVIEW), where: t('Changes requested on the PR.') };
  return { stage: stage(REVIEW, 'wait'), where: t('Waiting on a reviewer.') };
}

/** Assigned to me: its assignee is the one on my tickets (Now's list, every source) of that tracker. */
export function assignedToMe(item: Pick<TicketItem, 'ticket'>, mine: readonly TicketItem[] = []): boolean {
  const a = item.ticket.assignee;
  const me = (i: TicketItem): boolean =>
    i.ticket.ref.account === item.ticket.ref.account && i.ticket.assignee?.id === a?.id;
  return !!a && (mine.some(me) || tickets.items({ kind: 'all' }, null, 'mine').some(me));
}

/** `standing` from the stores: the ticket's live work item and its source's waiting statuses; without
 *  work, a Claude session Kelta did not start on the ticket (#345) puts it with Claude. */
export function standingNow(item: TicketItem, mine: boolean): Standing {
  // The ticket sits at its least advanced item (#456).
  const at = (w: WorkItem): number => workStage(w, phaseNow(w), prOf(w), false).at;
  const lagging = ticketWork(item.ticket.ref).reduce<WorkItem | null>(
    (a, w) => (!a || at(w) < at(a) ? w : a),
    null,
  );
  const w = lagging ?? (item.work_item_id ? work.get(item.work_item_id) : null);
  const live = w && w.state.kind !== 'finished' ? w : null;
  // The stored step (#462) places the ticket; its least advanced item and PR only say whose move.
  const step = stepOf(item.ticket.ref, item.source_ids[0] ?? null, live?.project_id ?? null);
  const key = ticketKey(item.ticket.ref);
  const outside = live ? null : sessions.elsewhere.find((o) => o.ticket && ticketKey(o.ticket) === key);
  if (outside && item.ticket.status.category !== 'done') {
    const phase = outsidePhase(outside);
    const yours = zoneOf(phase.section) === 'yours';
    const st = stage(CLAUDE, yours ? 'you' : 'wait');
    return {
      stage: step ? onStep(st, step, phase.lamp === 'working') : st,
      where: yours
        ? t('{phase}. Your move in the Queue.', { phase: phase.label })
        : t('{phase}.', { phase: phase.label }),
    };
  }
  return standing(
    item,
    live ? { item: live, phase: phaseNow(live), pr: prOf(live) } : null,
    mine,
    waits(item, waitingStatuses()),
    step,
  );
}

/** The PR state a row's second line shows (`#2291 checks failed`); `bad` when a check failed. */
export function prState(item: Pick<TicketItem, 'prs'>): { text: string; bad: boolean } | null {
  const pr = mainPr(item.prs);
  if (!pr) return null;
  const bad = pr.state === 'open' && (pr.ci === 'failure' || pr.ci === 'error');
  const word =
    pr.state === 'merged'
      ? t('merged')
      : pr.state === 'closed'
        ? t('closed')
        : bad
          ? t('checks failed')
          : pr.draft
            ? t('draft')
            : pr.review === 'approved'
              ? t('approved')
              : pr.review === 'changes_requested'
                ? t('changes requested')
                : t('review required');
  return { text: `${prLabel(pr)} ${word}`, bad };
}
