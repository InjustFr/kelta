// The one derived state of a work item (FLOW §2.2): label, lamp, next action and Now section.
// Pure; used by Now, the work bar, the tab lamp, the rail tile and the palette.

import type { GitStatus, Review, ReviewDelta, SessionInfo, WorkItem } from '$lib/gen';

/** Lamp shapes (DESIGN §6.1). `working` is derived, not an Attention level. */
export type Lamp = 'needs_input' | 'error' | 'working' | 'done' | 'activity' | 'none';

export type NowSection = 'needs_you' | 'to_review' | 'fix' | 'requests' | 'ship' | 'in_flight' | 'up_next';

/** Ids of the work action registry (`actions.ts`). */
export type WorkActionId =
  | 'retry'
  | 'skip_step'
  | 'recreate'
  | 'resolve'
  | 'go_claude'
  | 'review_diff'
  | 'review_delta'
  | 'mark_reviewed'
  | 'edit_note'
  | 'ship'
  | 'fix'
  | 'rebase'
  | 'rebase_continue'
  | 'rebase_abort'
  | 'conflicts'
  | 'link'
  | 'open_ticket'
  | 'open_pr'
  | 'open_review'
  | 'finish';

export type PhaseId =
  | 'failed'
  | 'starting'
  | 'missing'
  | 'rebase_stopped'
  | 'needs_you'
  | 'working'
  | 'merged'
  | 'closed'
  | 'to_review'
  | 'replied'
  | 'remote_new'
  | 'changes_requested'
  | 'checks_failed'
  | 'conflicts'
  | 'rebased'
  | 'unpushed'
  | 'ready'
  | 'approved'
  | 'in_review'
  | 'no_changes'
  | 'reviewing'
  | 'reviewed'
  | 'updated';

export interface Phase {
  id: PhaseId;
  label: string;
  detail: string;
  lamp: Lamp;
  primary: WorkActionId | null;
  /** Button text of the primary action ("Retry worktree", "Push", …). */
  primaryLabel: string;
  section: NowSection;
}

// Fields review-others adds (decision_head, the reviewed head). Optional until that lane lands them.
export type PhaseItem = WorkItem;
export type PhaseGit = GitStatus;
export type PhasePr = Review & {
  state?: 'open' | 'merged' | 'closed';
  decision_head?: string | null;
  /** Head of the PR when I last reviewed it. */
  reviewed_head?: string | null;
};

function p(
  id: PhaseId,
  label: string,
  detail: string,
  lamp: Lamp,
  primary: WorkActionId | null,
  primaryLabel: string,
  section: NowSection,
): Phase {
  return { id, label, detail, lamp, primary, primaryLabel, section };
}

const plural = (n: number, one: string): string => `${n} ${one}${n === 1 ? '' : 's'}`;

/** `1.8k` for big counts. */
const short = (n: number): string => (n >= 1000 ? `${(n / 1000).toFixed(1).replace(/\.0$/, '')}k` : `${n}`);

/** The delta chip parts: `212 lines`, `9 files`, `tests: none`, `+1.8k generated`. */
export function deltaParts(d: ReviewDelta): string[] {
  return [
    plural(d.lines, 'line').replace(/^\d+/, short(d.lines)),
    plural(d.files, 'file'),
    `tests: ${d.tests ? d.tests : 'none'}`,
    ...(d.generated ? [`+${short(d.generated)} generated`] : []),
  ];
}

/** Source files changed and no test touched: the chip's `tests: none` reads as a warning. */
export const testsMissing = (d: ReviewDelta): boolean => d.files > 0 && d.tests === 0;

export const deltaChip = (d: ReviewDelta): string => deltaParts(d).join(' · ');

/** A failed start step as a gerund for "Failed …" / "Retry …" (common.ts holds the checklist
 *  wording); unknown ids read as words. */
const STEP_DOING: Record<string, string> = {
  before_start: 'running the before-start hook',
  fetch_ticket: 'fetching the ticket',
  fetch_base: 'fetching the base branch',
  worktree: 'creating the branch folder',
  include_files: 'copying included files',
  claude_files: 'preparing Claude files',
  layout: 'opening the panes',
  setup: 'running setup',
  editor: 'starting the editor',
  claude: 'starting Claude',
  tracker_side_effects: 'updating the ticket',
  persist: 'saving the work item',
};
const doing = (step: string): string => STEP_DOING[step] ?? step.replace(/_/g, ' ');

/** The first matching row of FLOW §2.2 wins. `claude` is the item's Claude session (its `status`). */
export function phaseOf(
  item: PhaseItem,
  claude: SessionInfo | null,
  pr: PhasePr | null,
  git: PhaseGit | null,
): Phase {
  const st = item.state;
  const urlNo = item.pr_url?.match(/(\d+)\/?$/)?.[1];
  const prLabel = pr ? `#${pr.ref.number}` : urlNo ? `#${urlNo}` : '';
  if (st.kind === 'failed')
    return p(
      'failed',
      `Failed ${doing(st.step)}`,
      st.message,
      'error',
      'retry',
      `Retry ${doing(st.step)}`,
      'fix',
    );
  if (st.kind === 'starting' || st.kind === 'planned') {
    const done = item.steps.filter((s) => s.status === 'done' || s.status === 'skipped').length;
    const detail = item.steps.length
      ? `step ${Math.min(done + 1, item.steps.length)} of ${item.steps.length}`
      : '';
    return p('starting', 'Starting', detail, 'working', null, '', 'in_flight');
  }
  if (git?.missing)
    return p('missing', 'Worktree missing', item.worktree, 'error', 'recreate', 'Recreate', 'in_flight');
  const conflicts = item.rebase?.conflicts.length ?? 0;
  if (conflicts > 0)
    return p(
      'rebase_stopped',
      'Rebase stopped',
      plural(conflicts, 'conflicted file'),
      'error',
      'resolve',
      'Ask Claude to resolve',
      'fix',
    );
  if (claude?.status === 'needs_input')
    return p(
      'needs_you',
      'Claude needs you',
      claude.claude?.preview ?? '',
      'needs_input',
      'go_claude',
      'Go to Claude',
      'needs_you',
    );
  if (claude?.status === 'working')
    return p('working', 'Claude working', '', 'working', null, '', 'in_flight');
  if (st.kind === 'merged')
    return p('merged', 'Merged', st.detail ?? prLabel, 'none', 'finish', 'Finish…', 'ship');
  if (st.kind === 'pr_closed' || pr?.state === 'closed')
    return p('closed', 'PR closed', prLabel, 'none', 'finish', 'Finish…', 'ship');

  if (item.kind === 'review') {
    if (pr && pr.my_state && pr.my_state !== 'pending') {
      if (pr.reviewed_head && pr.reviewed_head !== pr.head_sha)
        return p(
          'updated',
          'Updated since your review',
          prLabel,
          'none',
          'open_review',
          'Open review',
          'requests',
        );
      return p('reviewed', 'Reviewed', prLabel, 'none', 'finish', 'Finish…', 'ship');
    }
    return p(
      'reviewing',
      `Reviewing ${prLabel}`.trim(),
      '',
      'none',
      'open_review',
      'Open review',
      'requests',
    );
  }

  if (item.review_due)
    return p(
      'to_review',
      'To review',
      item.delta ? deltaChip(item.delta) : 'Claude finished',
      'done',
      'review_delta',
      'Review changes',
      'to_review',
    );
  if (item.claude_replied)
    return p(
      'replied',
      'Claude replied',
      claude?.claude?.preview ?? '',
      'needs_input',
      'go_claude',
      'Go to Claude',
      'needs_you',
    );
  const hasPr = pr !== null || item.pr_url !== null;
  const remoteNew = git?.remote_new ?? 0;
  if (hasPr && remoteNew > 0)
    return p(
      'remote_new',
      'Remote has new commits',
      plural(remoteNew, 'commit'),
      'error',
      'rebase',
      `Rebase onto origin/${item.branch}`,
      'fix',
    );
  // Local work beats remote verdicts: only when the local HEAD is the PR head.
  const atPrHead = !git?.unpushed && !git?.dirty;
  if (pr && atPrHead) {
    const onHead = !pr.decision_head || pr.decision_head === pr.head_sha;
    if (pr.decision === 'changes_requested' && onHead)
      return p('changes_requested', 'Changes requested', prLabel, 'error', 'fix', 'Fix with Claude', 'fix');
    if (pr.ci === 'failure' || pr.ci === 'error')
      return p('checks_failed', 'Checks failed', prLabel, 'error', 'fix', 'Fix with Claude', 'fix');
  }
  if (pr?.mergeable === false)
    return p(
      'conflicts',
      `Conflicts with ${item.base}`,
      git ? `${git.behind} behind` : '',
      'error',
      'rebase',
      'Rebase',
      'fix',
    );
  if (hasPr && git?.diverged)
    return p('rebased', 'Rebased', `push rewrites ${prLabel}`, 'none', 'ship', 'Force push…', 'ship');
  if (hasPr && git?.unpushed) return p('unpushed', 'Unpushed', prLabel, 'none', 'ship', 'Push', 'ship');
  if (!hasPr && (git?.ahead ?? 0) > 0)
    return p('ready', 'Ready to ship', plural(git?.ahead ?? 0, 'commit'), 'none', 'ship', 'Ship', 'ship');
  if (pr?.decision === 'approved' && pr.ci === 'success')
    return p('approved', 'Approved', prLabel, 'none', 'open_pr', 'Open PR', 'ship');
  if (hasPr) {
    const ci = pr ? ` checks ${pr.ci}` : '';
    return p('in_review', 'In review', `${prLabel}${ci}`.trim(), 'none', 'open_pr', 'Open PR', 'in_flight');
  }
  return p('no_changes', 'No changes yet', '', 'none', 'go_claude', 'Go to Claude', 'in_flight');
}

const LAMP_RANK: Record<Lamp, number> = {
  none: 0,
  activity: 1,
  done: 2,
  working: 3,
  error: 4,
  needs_input: 5,
};

/** Aggregate lamp (DESIGN §6.1): needs input > error > working > done > activity. */
export function maxLamp(lamps: Iterable<Lamp>): Lamp {
  let best: Lamp = 'none';
  for (const l of lamps) if (LAMP_RANK[l] > LAMP_RANK[best]) best = l;
  return best;
}

/** Short id of an item: ticket key, `#n` for a review checkout, `wip` for scratch work. */
export function workKey(item: Pick<WorkItem, 'ticket' | 'review'>): string {
  if (item.ticket) return item.ticket.key;
  if (item.review) return `#${item.review.number}`;
  return 'wip';
}

/**
 * The return strip: Louis comes back to an item after `afterMins` away (0 = off) and there is
 * something to brief him on. Never while Claude is working.
 */
export function returnBrief(
  item: WorkItem,
  claudeStatus: string | null,
  now: number,
  afterMins: number,
): boolean {
  if (!afterMins || !item.left_at || claudeStatus === 'working') return false;
  if (!item.next_note && !item.claude_message && !item.delta) return false;
  return now - Date.parse(item.left_at) >= afterMins * 60_000;
}
