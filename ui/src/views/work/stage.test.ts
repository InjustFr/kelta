import { describe, expect, it } from 'vitest';

import { CLAUDE, DEPLOY, MERGE, PR, REVIEW, workStage } from './stage';

const item = { pr_url: null, review: null, feedback_at: null };

describe('workStage (#399)', () => {
  it('a diff to review sits with Claude, my move', () => {
    expect(workStage(item, { id: 'to_review', lamp: 'done' }, null, true)).toEqual({
      at: CLAUDE,
      reached: CLAUDE,
      mark: 'you',
    });
  });

  it('back with Claude after review: the line runs to Review, the mark on Claude', () => {
    const back = { ...item, pr_url: 'https://h/p/1', feedback_at: '2026-10-01T00:00:00Z' };
    expect(workStage(back, { id: 'to_review', lamp: 'done' }, null, true)).toEqual({
      at: CLAUDE,
      reached: REVIEW,
      mark: 'you',
    });
  });

  it('red checks are broken at PR; a PR in review waits at Review', () => {
    const pr = { ...item, pr_url: 'https://h/p/1' };
    expect(workStage(pr, { id: 'checks_failed', lamp: 'error' }, null, true)).toMatchObject({
      at: PR,
      mark: 'broken',
    });
    expect(workStage(pr, { id: 'in_review', lamp: 'none' }, null, false)).toEqual({
      at: REVIEW,
      reached: REVIEW,
      mark: 'wait',
    });
  });

  it('approved waits to merge; a ticket waiting for its deploy is at Deploy', () => {
    expect(workStage(item, { id: 'approved', lamp: 'none' }, { decision: 'approved' }, true).at).toBe(MERGE);
    expect(workStage(item, { id: 'merged', lamp: 'none' }, null, false, true)).toEqual({
      at: DEPLOY,
      reached: DEPLOY,
      mark: 'wait',
    });
  });
});
