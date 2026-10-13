// The one derived state of a work item (FLOW §2.2): label, lamp, next action and Now section.
// Pure; used by Now, the work bar, the tab lamp, the rail tile and the palette.

import { msg, t, tn } from '$lib/i18n/index.svelte';
import type {
  ClaudeQuestion,
  GitStatus,
  OutsideSession,
  Review,
  ReviewDelta,
  RunState,
  SessionInfo,
  WorkItem,
} from '$lib/gen';

/** Lamp shapes (DESIGN §6.1). `working` and `parked` are derived, not Attention levels. */
export type Lamp = 'needs_input' | 'error' | 'working' | 'done' | 'activity' | 'parked' | 'none';

export type NowSection =
  | 'needs_you'
  | 'to_review'
  | 'next_up'
  | 'new'
  | 'fix'
  | 'requests'
  | 'ship'
  | 'in_flight'
  | 'elsewhere'
  | 'blocked'
  | 'up_next';

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
  | 'fix_checks'
  | 'rebase'
  | 'rebase_continue'
  | 'rebase_abort'
  | 'conflicts'
  | 'link'
  | 'create_ticket'
  | 'open_ticket'
  | 'open_pr'
  | 'open_review'
  | 'merge'
  | 'set_draft'
  | 'start_now'
  | 'queue_front'
  | 'park'
  | 'finish';

export type PhaseId =
  | 'asking'
  | 'run'
  | 'failed'
  | 'starting'
  | 'queued'
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
  | 'new_comments'
  | 'conflicts'
  | 'rebased'
  | 'unpushed'
  | 'ready'
  | 'approved'
  | 'draft'
  | 'in_review'
  | 'no_changes'
  | 'reviewing'
  | 'reviewed'
  | 'updated'
  | 'elsewhere';

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

/** The open MCP `ask` question of a live session (#356). */
export const questionOf = (s: SessionInfo | null | undefined): ClaudeQuestion | null =>
  (s?.lifecycle === 'live' && s.claude?.question) || null;

/** `1.8k` for big counts. */
const short = (n: number): string => (n >= 1000 ? `${(n / 1000).toFixed(1).replace(/\.0$/, '')}k` : `${n}`);

/** The delta chip parts: `212 lines`, `9 files`, `tests: none`, `+1.8k generated`. */
export function deltaParts(d: ReviewDelta): string[] {
  return [
    tn('{n} line', '{n} lines', d.lines, { n: short(d.lines) }),
    tn('{n} file', '{n} files', d.files),
    d.tests ? t('tests: {n}', { n: d.tests }) : t('tests: none'),
    ...(d.generated ? [t('+{n} generated', { n: short(d.generated) })] : []),
  ];
}

/** Source files changed and no test touched: the chip's `tests: none` reads as a warning. */
export const testsMissing = (d: ReviewDelta): boolean => d.files > 0 && d.tests === 0;

export const deltaChip = (d: ReviewDelta): string => deltaParts(d).join(' · ');

/** A failed start step as a gerund for "Failed …" / "Retry …" (common.ts holds the checklist
 *  wording); unknown ids read as words. */
const STEP_DOING: Record<string, string> = {
  before_start: msg('running the before-start hook'),
  fetch_ticket: msg('fetching the ticket'),
  fetch_base: msg('fetching the base branch'),
  worktree: msg('creating the branch folder'),
  include_files: msg('copying included files'),
  claude_files: msg('preparing Claude files'),
  layout: msg('opening the panes'),
  setup: msg('running setup'),
  editor: msg('starting the editor'),
  claude: msg('starting Claude'),
  tracker_side_effects: msg('updating the ticket'),
  persist: msg('saving the work item'),
};
const doing = (step: string): string => (STEP_DOING[step] ? t(STEP_DOING[step]) : step.replace(/_/g, ' '));

/** The first matching row of FLOW §2.2 wins. `claude` is the item's Claude session (its `status`).
 *  A parked item (#142) keeps its phase; its quiet lamp (none, activity, done) turns `parked`. */
export function phaseOf(
  item: PhaseItem,
  claude: SessionInfo | null,
  pr: PhasePr | null,
  git: PhaseGit | null,
): Phase {
  const ph = phaseRow(item, claude, pr, git);
  const parked = item.parked_at && claude?.lifecycle !== 'live' && LAMP_RANK[ph.lamp] <= LAMP_RANK.done;
  return parked ? { ...ph, lamp: 'parked' } : ph;
}

function phaseRow(
  item: PhaseItem,
  claude: SessionInfo | null,
  pr: PhasePr | null,
  git: PhaseGit | null,
): Phase {
  // `claude` is the asking session when one asks (`phaseNow`): its question beats the run's phase.
  const q = questionOf(claude);
  if (q) return p('asking', q.question, '', 'needs_input', 'go_claude', t('Go to Claude'), 'needs_you');
  const run = item.run;
  // A background run's claim (#235): its own phase and state, `note` as the detail.
  if (run && run.state !== 'done') {
    const name = [run.name, run.phase].filter(Boolean).join(' · ') || t('Background run');
    const stuck = run.state === 'blocked' || run.state === 'failed';
    return p(
      'run',
      run.state === 'orphaned' ? t('{name} (orphaned)', { name }) : name,
      run.note ?? t(RUN_STATE[run.state]),
      RUN_LAMP[run.state],
      null,
      '',
      stuck ? 'needs_you' : 'in_flight',
    );
  }
  const st = item.state;
  const urlNo = item.pr_url?.match(/(\d+)\/?$/)?.[1];
  const prLabel = pr ? `#${pr.ref.number}` : urlNo ? `#${urlNo}` : '';
  if (st.kind === 'failed')
    return p(
      'failed',
      t('Failed {step}', { step: doing(st.step) }),
      st.message,
      'error',
      'retry',
      t('Retry {step}', { step: doing(st.step) }),
      'fix',
    );
  if (st.kind === 'queued')
    return p(
      'queued',
      t('Queued'),
      t('waiting for a Claude slot'),
      'none',
      'start_now',
      t('Start now (over cap)'),
      'in_flight',
    );
  if (st.kind === 'starting' || st.kind === 'planned') {
    const done = item.steps.filter((s) => s.status === 'done' || s.status === 'skipped').length;
    const detail = item.steps.length
      ? t('step {n} of {total}', { n: Math.min(done + 1, item.steps.length), total: item.steps.length })
      : '';
    return p('starting', t('Starting'), detail, 'working', null, '', 'in_flight');
  }
  if (git?.missing)
    return p(
      'missing',
      t('Worktree missing'),
      item.worktree,
      'error',
      'recreate',
      t('Recreate'),
      'in_flight',
    );
  const conflicts = item.rebase?.conflicts.length ?? 0;
  if (conflicts > 0)
    return p(
      'rebase_stopped',
      t('Rebase stopped'),
      tn('{n} conflicted file', '{n} conflicted files', conflicts),
      'error',
      'resolve',
      t('Ask Claude to resolve'),
      'fix',
    );
  if (claude?.status === 'needs_input')
    return p(
      'needs_you',
      t('Claude needs you'),
      claude.claude?.preview ?? '',
      'needs_input',
      'go_claude',
      t('Go to Claude'),
      'needs_you',
    );
  if (claude?.status === 'working')
    return p('working', t('Claude working'), '', 'working', null, '', 'in_flight');
  if (st.kind === 'merged')
    return p('merged', t('Merged'), st.detail ?? prLabel, 'none', 'finish', t('Finish…'), 'ship');
  if (st.kind === 'pr_closed' || pr?.state === 'closed')
    return p('closed', t('PR closed'), prLabel, 'none', 'finish', t('Finish…'), 'ship');

  if (item.kind === 'review') {
    if (pr && pr.my_state && pr.my_state !== 'pending') {
      if (pr.reviewed_head && pr.reviewed_head !== pr.head_sha)
        return p(
          'updated',
          t('Updated since your review'),
          prLabel,
          'none',
          'open_review',
          t('Open review'),
          'requests',
        );
      return p('reviewed', t('Reviewed'), prLabel, 'none', 'finish', t('Finish…'), 'ship');
    }
    return p(
      'reviewing',
      t('Reviewing {pr}', { pr: prLabel }).trim(),
      '',
      'none',
      'open_review',
      t('Open review'),
      'requests',
    );
  }

  if (item.review_due)
    return p(
      'to_review',
      t('To review'),
      item.delta ? deltaChip(item.delta) : t('Claude finished'),
      'done',
      'review_delta',
      t('Review changes'),
      'to_review',
    );
  if (item.claude_replied)
    return p(
      'replied',
      t('Claude replied'),
      claude?.claude?.preview ?? '',
      'needs_input',
      'go_claude',
      t('Go to Claude'),
      'needs_you',
    );
  const hasPr = pr !== null || item.pr_url !== null;
  const remoteNew = git?.remote_new ?? 0;
  if (hasPr && remoteNew > 0)
    return p(
      'remote_new',
      t('Remote has new commits'),
      tn('{n} commit', '{n} commits', remoteNew),
      'error',
      'rebase',
      t('Rebase onto origin/{branch}', { branch: item.branch }),
      'fix',
    );
  // Local work beats remote verdicts: only when the local HEAD is the PR head.
  const atPrHead = !git?.unpushed && !git?.dirty;
  if (pr && atPrHead) {
    const onHead = !pr.decision_head || pr.decision_head === pr.head_sha;
    const red = pr.ci === 'failure' || pr.ci === 'error';
    const names = pr.failed_check_names;
    const shown = names.slice(0, 3).join(', ') + (names.length > 3 ? ` +${names.length - 3}` : '');
    const detail = red && shown ? `${prLabel} · ${shown}` : prLabel;
    // CI and review are separate axes (#369): the label names the other one too.
    if (pr.decision === 'changes_requested' && onHead)
      return p(
        'changes_requested',
        red ? `${t('Changes requested')} · ${t('checks failed')}` : t('Changes requested'),
        detail,
        'error',
        'fix',
        t('Fix with Claude'),
        'fix',
      );
    if (red) {
      const review = reviewOf(pr);
      return p(
        'checks_failed',
        review ? `${t('Checks failed')} · ${review}` : t('Checks failed'),
        detail,
        'error',
        'fix_checks',
        t('Send failures to Claude'),
        'fix',
      );
    }
  }
  // Teammates commented since the last Fix with Claude (#208); unpushed work does not hide it.
  const seen = item.feedback_at ? Date.parse(item.feedback_at) : -Infinity;
  if (pr?.last_comment_at && Date.parse(pr.last_comment_at) > seen)
    return p(
      'new_comments',
      t('New comments'),
      tn('{n} unresolved thread', '{n} unresolved threads', pr.unresolved_threads ?? 0),
      'error',
      'fix',
      t('Fix with Claude'),
      'fix',
    );
  if (pr?.mergeable === false)
    return p(
      'conflicts',
      t('Conflicts with {base}', { base: item.base }),
      git ? t('{n} behind', { n: git.behind }) : '',
      'error',
      'rebase',
      t('Rebase'),
      'fix',
    );
  if (hasPr && git?.diverged)
    return p(
      'rebased',
      t('Rebased'),
      t('push rewrites {pr}', { pr: prLabel }),
      'none',
      'ship',
      t('Force push…'),
      'ship',
    );
  if (hasPr && git?.unpushed) return p('unpushed', t('Unpushed'), prLabel, 'none', 'ship', t('Push'), 'ship');
  if (!hasPr && (git?.ahead ?? 0) > 0)
    return p(
      'ready',
      t('Ready to ship'),
      tn('{n} commit', '{n} commits', git?.ahead ?? 0),
      'none',
      'ship',
      t('Ship'),
      'ship',
    );
  // Pushed (the rows above catch unpushed work) and CI not red: ready to leave draft.
  if (pr?.draft) {
    const red = pr.ci === 'failure' || pr.ci === 'error';
    return red
      ? p(
          'draft',
          t('Draft'),
          `${prLabel} ${t('checks {state}', { state: pr.ci })}`,
          'none',
          'open_pr',
          t('Open PR'),
          'in_flight',
        )
      : p('draft', t('Draft'), prLabel, 'none', 'set_draft', t('Mark ready…'), 'ship');
  }
  if (pr?.decision === 'approved' && pr.ci === 'success')
    return p('approved', t('Approved'), prLabel, 'none', 'open_pr', t('Open PR'), 'ship');
  if (hasPr) {
    const ci = pr ? ` ${t('checks {state}', { state: pr.ci })}` : '';
    return p(
      'in_review',
      t('In review'),
      [`${prLabel}${ci}`.trim(), pr ? reviewOf(pr) : ''].filter(Boolean).join(' · '),
      'none',
      'open_pr',
      t('Open PR'),
      'in_flight',
    );
  }
  return p('no_changes', t('No changes yet'), '', 'none', 'go_claude', t('Go to Claude'), 'in_flight');
}

/** The phase as one word a list row can show (#373): who holds the item, '' while it waits on others. */
export function phaseWord(ph: Phase): string {
  if (ph.lamp === 'parked') return t('Resume');
  if (ph.section === 'needs_you') return t('Needs you');
  if (ph.lamp === 'working') return t('Working');
  const mine = ph.lamp === 'error' || ph.lamp === 'done' || YOUR_TURN.includes(ph.section);
  return mine ? t('Your turn') : '';
}

const YOUR_TURN: NowSection[] = ['fix', 'to_review', 'ship', 'requests'];

/** The review side of a PR, shown beside its CI (#369); '' when no review is asked for. A
 *  changes-requested review on an older commit waits for re-review. */
export function reviewOf(pr: PhasePr): string {
  const onHead = !pr.decision_head || pr.decision_head === pr.head_sha;
  if (pr.decision === 'approved') return t('approved');
  if (pr.decision === 'changes_requested' && onHead) return t('changes requested');
  return pr.decision ? t('review required') : '';
}

const RUN_STATE: Record<RunState, string> = {
  running: msg('running'),
  blocked: msg('blocked'),
  failed: msg('failed'),
  done: msg('done'),
  orphaned: msg('orphaned'),
};

// Blocked is amber (the `error` lamp), failed red.
const RUN_LAMP: Record<RunState, Lamp> = {
  running: 'working',
  blocked: 'error',
  failed: 'needs_input',
  done: 'none',
  orphaned: 'none',
};

const LAMP_RANK: Record<Lamp, number> = {
  none: 0,
  parked: 0.5,
  activity: 1,
  done: 2,
  working: 3,
  error: 4,
  needs_input: 5,
};

/** Aggregate lamp (DESIGN §6.1): needs input > error > working > done > activity. */
/** A Claude session Kelta did not start (#345): its lamp and words, in Now and on its ticket's row. */
export function outsidePhase(s: Pick<OutsideSession, 'status' | 'preview'>): Phase {
  const [lamp, label]: [Lamp, string] =
    s.status === 'needs_input'
      ? ['needs_input', t('Claude needs you, outside Kelta')]
      : s.status === 'working'
        ? ['working', t('Claude working outside Kelta')]
        : s.status === 'error'
          ? ['error', t('Claude stopped on an error, outside Kelta')]
          : s.status === 'done' || s.status === 'waiting_user'
            ? ['done', t('Claude done, outside Kelta')]
            : ['none', t('Claude open outside Kelta')];
  const section = s.status === 'needs_input' ? 'needs_you' : 'elsewhere';
  return p('elsewhere', label, s.preview ?? '', lamp, null, '', section);
}

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
