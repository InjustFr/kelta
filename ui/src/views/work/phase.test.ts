import { describe, expect, it } from 'vitest';

import * as samples from '$lib/gen/fixtures';

import {
  deltaChip,
  maxLamp,
  phaseOf,
  testsMissing,
  type PhaseGit,
  type PhaseItem,
  type PhasePr,
} from './phase';

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
    expect(phaseOf(failed, null, null, null).primaryLabel).toBe('Retry creating the branch folder');
    expect(phaseOf(failed, null, null, null).label).toBe('Failed creating the branch folder');
    const queued = phaseOf(item({ state: { kind: 'queued', pos: 0 } }), null, null, null);
    check('queued', queued, 'queued', 'in_flight', 'start_now');
    expect([queued.label, queued.lamp]).toEqual(['Queued', 'none']);
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
      phaseOf(item({ state: { kind: 'merged', detail: null }, review_due: true }), null, null, null),
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
      '7 closed (state)',
      phaseOf(item({ ...withPr, state: { kind: 'pr_closed' } }), null, null, null),
      'closed',
      'ship',
      'finish',
    );
    check(
      '8 to review',
      phaseOf(item({ review_due: true, claude_replied: true }), claude('done'), null, git()),
      'to_review',
      'to_review',
      'review_delta',
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
      'fix_checks',
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
    check(
      '16b draft, pushed and green',
      phaseOf(item(withPr), null, pr({ draft: true, ci: 'pending' }), git()),
      'draft',
      'ship',
      'set_draft',
    );
    check(
      '16b draft with red CI (local edits hide Checks failed)',
      phaseOf(item(withPr), null, pr({ draft: true, ci: 'failure' }), git({ dirty: true })),
      'draft',
      'in_flight',
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

  it('Checks failed sends the failures and names up to 3 checks', () => {
    const n = samples.reviewDetail.review.ref.number;
    const failed = (names: string[]) =>
      phaseOf(item(withPr), null, pr({ ci: 'error', failed_check_names: names }), git());
    expect([failed([]).primaryLabel, failed([]).detail]).toEqual(['Send failures to Claude', `#${n}`]);
    expect(failed(['test (ubuntu)', 'lint']).detail).toBe(`#${n} · test (ubuntu), lint`);
    expect(failed(['a', 'b', 'c', 'd', 'e']).detail).toBe(`#${n} · a, b, c +2`);
  });

  it('shows CI and review side by side (#369)', () => {
    const n = samples.reviewDetail.review.ref.number;
    const at = (patch: Partial<PhasePr>) => phaseOf(item(withPr), null, pr(patch), git());
    const awaiting = at({ ci: 'failure', decision: 'review_required', failed_check_names: ['e2e'] });
    expect([awaiting.id, awaiting.label, awaiting.detail, awaiting.primary]).toEqual([
      'checks_failed',
      'Checks failed · review required',
      `#${n} · e2e`,
      'fix_checks',
    ]);
    expect(at({ ci: 'failure', decision: 'approved' }).label).toBe('Checks failed · approved');
    expect(at({ ci: 'failure', decision: null }).label).toBe('Checks failed');
    const both = at({ ci: 'failure', decision: 'changes_requested', failed_check_names: ['e2e'] });
    expect([both.id, both.label, both.detail]).toEqual([
      'changes_requested',
      'Changes requested · checks failed',
      `#${n} · e2e`,
    ]);
    expect(at({ decision: 'changes_requested' }).detail).toBe(`#${n}`);
    expect(at({ ci: 'pending', decision: 'review_required' }).detail).toBe(
      `#${n} checks pending · review required`,
    );
    // A changes-requested review on an older commit waits for re-review.
    const stale = at({ ci: 'failure', decision: 'changes_requested', head_sha: 'b', decision_head: 'a' });
    expect([stale.id, stale.label]).toEqual(['checks_failed', 'Checks failed · review required']);
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

describe('delta chip', () => {
  const d = { lines: 212, files: 9, tests: 0, generated: 1800, insertions: 1890, deletions: 122 };
  it('reads lines, files, tests and generated lines apart', () => {
    expect(deltaChip(d)).toBe('212 lines · 9 files · tests: none · +1.8k generated');
    expect(deltaChip({ ...d, lines: 1, files: 1, tests: 1, generated: 0 })).toBe(
      '1 line · 1 file · tests: 1',
    );
    expect(phaseOf(item({ review_due: true, delta: d }), claude('done'), null, git()).detail).toBe(
      deltaChip(d),
    );
  });
  it('warns when source changed without tests', () => {
    expect(testsMissing(d)).toBe(true);
    expect(testsMissing({ ...d, tests: 2 })).toBe(false);
    expect(testsMissing({ ...d, files: 0 })).toBe(false);
  });
});

describe('phaseOf: parked (#142)', () => {
  const parked = item({ review_due: true, parked_at: '2026-10-10T08:00:00Z' });
  const dormant = { ...claude('unknown'), lifecycle: 'dormant' } as typeof samples.sessionInfo;

  it('keeps the phase, the quiet lamp turns parked', () => {
    const got = phaseOf(parked, dormant, null, null);
    expect([got.id, got.lamp]).toEqual(['to_review', 'parked']);
    expect(maxLamp(['none', 'parked'])).toBe('parked');
    expect(maxLamp(['parked', 'activity'])).toBe('activity');
  });

  it('a louder lamp or a running Claude wins', () => {
    const failed = item({ parked_at: parked.parked_at, state: { kind: 'failed', step: 'x', message: 'y' } });
    expect(phaseOf(failed, dormant, null, null).lamp).toBe('error');
    expect(phaseOf(parked, claude('done'), null, null).lamp).toBe('done');
  });
});

describe('background run claims (#235)', () => {
  const run = (state: 'running' | 'blocked' | 'failed' | 'done' | 'orphaned', note: string | null = null) =>
    item({
      session_ids: [],
      run: {
        name: 'kelta-ticket-pass',
        run_id: 'r1',
        session_id: 'orch',
        phase: 'Review',
        state,
        note,
        sha: null,
      },
    });

  it('shows the run and its phase; state drives the lamp and the Now section', () => {
    const ph = phaseOf(run('running'), claude('needs_input'), null, git({ missing: true }));
    expect([ph.id, ph.label, ph.lamp, ph.section]).toEqual([
      'run',
      'kelta-ticket-pass · Review',
      'working',
      'in_flight',
    ]);
    const blocked = phaseOf(run('blocked', '2 unresolved review findings'), null, null, null);
    expect([blocked.lamp, blocked.section, blocked.detail]).toEqual([
      'error',
      'needs_you',
      '2 unresolved review findings',
    ]);
    expect(phaseOf(run('failed'), null, null, null)).toMatchObject({
      lamp: 'needs_input',
      section: 'needs_you',
    });
    expect(phaseOf(run('orphaned'), null, null, null)).toMatchObject({
      label: 'kelta-ticket-pass · Review (orphaned)',
      lamp: 'none',
    });
    expect(phaseOf(run('done'), null, null, null).id).not.toBe('run');
  });

  it("an `ask` question (#356) beats the run's phase while its session is live", () => {
    const question = { question: 'Which one?', options: ['A', 'B'], body_html: null };
    const asker = { ...claude('waiting_user'), claude: { ...samples.sessionInfo.claude!, question } };
    expect(phaseOf(run('running'), asker, null, null)).toMatchObject({
      id: 'asking',
      label: 'Which one?',
      lamp: 'needs_input',
      section: 'needs_you',
    });
    expect(phaseOf(run('running'), { ...asker, lifecycle: 'exited' }, null, null).id).toBe('run');
  });
});

describe('New comments (#208): the Fix with Claude watermark', () => {
  const commented = pr({ last_comment_at: '2026-10-10T09:00:00.000Z', unresolved_threads: 3 });

  it('fires when a comment is newer than the last send, or nothing was ever sent', () => {
    const got = phaseOf(item(withPr), claude('done'), commented, git());
    expect([got.id, got.section, got.primary, got.detail]).toEqual([
      'new_comments',
      'fix',
      'fix',
      '3 unresolved threads',
    ]);
    // 10:30+02:00 is 08:30Z: older as an instant, though later as a string.
    const before = item({ ...withPr, feedback_at: '2026-10-10T10:30:00+02:00' });
    expect(phaseOf(before, claude('done'), commented, git()).id).toBe('new_comments');
  });

  it('stays quiet once the comments are sent, or without any', () => {
    const sent = item({ ...withPr, feedback_at: '2026-10-10T09:00:00Z' });
    expect(phaseOf(sent, claude('done'), commented, git()).id).toBe('in_review');
    expect(phaseOf(item(withPr), claude('done'), pr(), git()).id).toBe('in_review');
  });

  it('To review and Changes requested on the head beat it; unpushed work does not hide it', () => {
    const due = item({ ...withPr, review_due: true });
    expect(phaseOf(due, claude('done'), commented, git()).id).toBe('to_review');
    const requested = { ...commented, decision: 'changes_requested' as const, decision_head: null };
    expect(phaseOf(item(withPr), claude('done'), requested, git()).id).toBe('changes_requested');
    expect(phaseOf(item(withPr), claude('done'), commented, git({ unpushed: true, ahead: 1 })).id).toBe(
      'new_comments',
    );
  });
});
