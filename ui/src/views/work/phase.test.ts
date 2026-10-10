import { describe, expect, it } from 'vitest';

import * as samples from '$lib/gen/fixtures';

import { maxLamp, phaseOf, type PhaseGit, type PhaseItem, type PhasePr } from './phase';

const item = (patch: Partial<PhaseItem> = {}): PhaseItem => ({ ...samples.workItem, ...patch });
const git = (patch: Partial<PhaseGit> = {}): PhaseGit => ({
  ahead: 0,
  behind: 0,
  dirty: false,
  unpushed: false,
  files: 0,
  insertions: 0,
  deletions: 0,
  missing: false,
  ...patch,
});
const pr = (patch: Partial<PhasePr> = {}): PhasePr => ({
  ...samples.reviewDetail.review,
  kind: 'authored',
  decision: null,
  ci: 'pending',
  mergeable: true,
  my_state: null,
  ...patch,
});
const claude = (status: string) => ({ ...samples.sessionInfo, status }) as typeof samples.sessionInfo;
const withPr = { pr_url: 'https://github.com/acme/shop-api/pull/311', state: { kind: 'pr_open' as const } };

describe('phaseOf: the 18 rows of FLOW §2.2', () => {
  const check = (
    name: string,
    got: ReturnType<typeof phaseOf>,
    id: string,
    section: string,
    primary: string | null,
  ) => {
    expect([got.id, got.section, got.primary], name).toEqual([id, section, primary]);
  };

  it('picks the first matching row', () => {
    const failed = item({ state: { kind: 'failed', step: 'worktree', message: 'exists' } });
    check('1 failed', phaseOf(failed, claude('needs_input'), null, null), 'failed', 'fix', 'retry');
    expect(phaseOf(failed, null, null, null).primaryLabel).toBe('Retry worktree');
    check(
      '2 starting',
      phaseOf(item({ state: { kind: 'starting' } }), null, null, null),
      'starting',
      'in_flight',
      null,
    );
    check(
      '3 rebase stopped',
      phaseOf(
        item({
          rebase: {
            onto: 'origin/main',
            pre_head: 'a',
            remote_sha: null,
            conflicts: ['a.rs', 'b.rs'],
            step: 1,
            total: 2,
          },
        }),
        claude('needs_input'),
        null,
        null,
      ),
      'rebase_stopped',
      'fix',
      'resolve',
    );
    check(
      '4 needs you',
      phaseOf(item({ review_due: true }), claude('needs_input'), null, null),
      'needs_you',
      'needs_you',
      'go_claude',
    );
    check(
      '5 working',
      phaseOf(item({ review_due: true }), claude('working'), null, null),
      'working',
      'in_flight',
      null,
    );
    check(
      '6 merged',
      phaseOf(item({ state: { kind: 'merged' }, review_due: true }), null, null, null),
      'merged',
      'ship',
      'finish',
    );
    check(
      '7 closed',
      phaseOf(item({ ...withPr, review_due: true }), null, pr({ state: 'closed' }), null),
      'closed',
      'ship',
      'finish',
    );
    check(
      '8 to review',
      phaseOf(item({ review_due: true, claude_replied: true }), claude('done'), null, git()),
      'to_review',
      'to_review',
      'review_diff',
    );
    check(
      '9 replied',
      phaseOf(item({ claude_replied: true }), claude('done'), null, git({ ahead: 2 })),
      'replied',
      'needs_you',
      'go_claude',
    );
    check(
      '10 remote new',
      phaseOf(item(withPr), null, pr({ decision: 'changes_requested' }), git({ remote_new: 2 })),
      'remote_new',
      'fix',
      'rebase',
    );
    check(
      '11 changes requested',
      phaseOf(item(withPr), null, pr({ decision: 'changes_requested' }), git()),
      'changes_requested',
      'fix',
      'fix',
    );
    check(
      '11 checks failed',
      phaseOf(item(withPr), null, pr({ ci: 'failure' }), git()),
      'checks_failed',
      'fix',
      'fix',
    );
    check(
      '12 conflicts',
      phaseOf(item(withPr), null, pr({ mergeable: false }), git({ behind: 3 })),
      'conflicts',
      'fix',
      'rebase',
    );
    check(
      '13 rebased',
      phaseOf(item(withPr), null, pr(), git({ diverged: true, unpushed: true })),
      'rebased',
      'ship',
      'ship',
    );
    expect(phaseOf(item(withPr), null, pr(), git({ diverged: true })).primaryLabel).toBe('Force push…');
    check(
      '14 unpushed',
      phaseOf(item(withPr), null, pr(), git({ unpushed: true })),
      'unpushed',
      'ship',
      'ship',
    );
    check('15 ready to ship', phaseOf(item(), null, null, git({ ahead: 2 })), 'ready', 'ship', 'ship');
    check(
      '16 approved',
      phaseOf(item(withPr), null, pr({ decision: 'approved', ci: 'success' }), git()),
      'approved',
      'ship',
      'open_pr',
    );
    check('17 in review', phaseOf(item(withPr), null, pr(), git()), 'in_review', 'in_flight', 'open_pr');
    check(
      '17 in review without the PR loaded',
      phaseOf(item(withPr), null, null, null),
      'in_review',
      'in_flight',
      'open_pr',
    );
    check(
      '18 no changes',
      phaseOf(item(), claude('done'), null, git()),
      'no_changes',
      'in_flight',
      'go_claude',
    );
  });

  it('a missing worktree offers Recreate', () => {
    const got = phaseOf(item(), null, null, git({ missing: true }));
    expect([got.id, got.primary, got.lamp]).toEqual(['missing', 'recreate', 'error']);
  });

  it('waiting_user (idle) never means needs you', () => {
    expect(phaseOf(item(), claude('waiting_user'), null, git()).section).toBe('in_flight');
  });
});

describe('Flow 2 ordering (FLOW §2.2 notes)', () => {
  const requested = pr({ decision: 'changes_requested', head_sha: 'aaa', decision_head: 'aaa' });

  it('changes requested + review_due + unpushed reads To review', () => {
    const got = phaseOf(
      item({ ...withPr, review_due: true }),
      claude('done'),
      requested,
      git({ unpushed: true }),
    );
    expect(got.id).toBe('to_review');
  });

  it('after Mark reviewed it reads Unpushed, primary Push', () => {
    const got = phaseOf(item(withPr), claude('done'), requested, git({ unpushed: true }));
    expect([got.id, got.primaryLabel]).toEqual(['unpushed', 'Push']);
  });

  it('after the push, a decisive review on an older commit reads In review', () => {
    const pushed = { ...requested, head_sha: 'bbb' };
    expect(phaseOf(item(withPr), claude('done'), pushed, git()).id).toBe('in_review');
  });
});

describe('review checkouts', () => {
  const review = item({ kind: 'review', ticket: null, review: samples.reviewDetail.review.ref });

  it('reads Reviewing, then Reviewed, then Updated since your review', () => {
    expect(phaseOf(review, null, pr({ my_state: 'pending' }), git()).id).toBe('reviewing');
    expect(phaseOf(review, null, pr({ my_state: 'approved' }), git()).primary).toBe('finish');
    const moved = pr({ my_state: 'approved', reviewed_head: 'old', head_sha: 'new' });
    expect(phaseOf(review, null, moved, git()).section).toBe('requests');
  });

  it('never reads To review: rows 1-5 then the review phases', () => {
    expect(phaseOf({ ...review, review_due: true }, null, null, git()).id).toBe('reviewing');
    expect(phaseOf(review, claude('needs_input'), null, git()).id).toBe('needs_you');
  });
});

describe('maxLamp', () => {
  it('ranks needs input > error > working > done > activity', () => {
    expect(maxLamp(['done', 'working', 'activity'])).toBe('working');
    expect(maxLamp(['error', 'needs_input'])).toBe('needs_input');
    expect(maxLamp([])).toBe('none');
  });
});
