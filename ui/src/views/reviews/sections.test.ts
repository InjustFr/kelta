import { describe, expect, it } from 'vitest';

import type { Review, ReviewItem } from '$lib/gen';
import { reviewPage } from '$lib/gen/fixtures';

import { beforeMerging } from './beforeMerging';
import { areaOf, barOf, changeMap, filesByArea, kLines } from './changemap';
import { openDays, ownStep, reviewState, rowState, sectionItems, stackOrder } from './sections';

function pr(number: number, over: Partial<Review> = {}): ReviewItem {
  const s = reviewPage.items[0]!;
  return {
    ...s,
    review: {
      ...s.review,
      ref: { ...s.review.ref, number },
      source_branch: `b${number}`,
      target_branch: 'main',
      draft: false,
      reviewers: [],
      waiting_on: [],
      decision: null,
      ...over,
    },
  };
}

const who = (state: 'approved' | 'changes_requested' | 'commented') => ({
  user: { id: state, name: state, login: state, avatar_url: null },
  state,
});

describe('stacks', () => {
  it('orders a stack in merge order, deeper each step, and says where each PR merges', () => {
    const [a, b, c, d] = [pr(1), pr(2, { target_branch: 'b1' }), pr(3, { target_branch: 'b2' }), pr(4)];
    const s = stackOrder([c, d, b, a]);
    expect(s.map((x) => [x.item.review.ref.number, x.depth])).toEqual([
      [4, 0],
      [1, 0],
      [2, 1],
      [3, 2],
    ]);
    expect(s.map(rowState)).toEqual([
      'No review yet, nobody requested',
      'No review yet, nobody requested, base of a stack of 3',
      'No review yet, nobody requested, merges into #1',
      'No review yet, nobody requested, merges into #2',
    ]);
  });

  it('keeps every PR of a cycle of bases once', () => {
    const s = stackOrder([pr(1, { target_branch: 'b2' }), pr(2, { target_branch: 'b1' })]);
    expect(s.map((x) => x.item.review.ref.number).sort()).toEqual([1, 2]);
  });
});

describe('sections', () => {
  it('puts the team PRs that are not mine nor asked of me in Team or Team drafts', () => {
    const s = sectionItems([pr(1)], [pr(1), pr(2), pr(3, { draft: true }), pr(4)], [pr(4)]);
    const n = (l: ReviewItem[]) => l.map((i) => i.review.ref.number);
    expect([n(s.asked), n(s.team), n(s.drafts), n(s.mine)]).toEqual([[1], [2], [3], [4]]);
  });
});

describe('reviewState', () => {
  it('leads with what reviewers did, then who is still asked', () => {
    expect(reviewState(pr(1).review)).toBe('No review yet, nobody requested');
    expect(reviewState(pr(1, { waiting_on: ['a', 'b', 'c', 'd'] }).review)).toBe(
      '4 reviewers requested, none reviewed',
    );
    expect(reviewState(pr(1, { reviewers: [who('approved')], waiting_on: ['a', 'b'] }).review)).toBe(
      '1 approval, 2 reviewers still requested',
    );
    expect(reviewState(pr(1, { reviewers: [who('commented')] }).review)).toBe('1 comment, nobody requested');
    expect(reviewState(pr(1, { decision: 'approved' }).review)).toBe('Approved, nobody requested');
    expect(rowState({ item: pr(1, { draft: true }), depth: 0, parent: null, size: 1 })).toBe('Draft');
  });

  it('ages a PR from its opening, else its last update', () => {
    const now = Date.parse('2026-10-13T12:00:00Z');
    expect(openDays({ created_at: '2026-09-29T12:00:00Z', updated_at: '2026-10-13T00:00:00Z' }, now)).toBe(
      14,
    );
    expect(openDays({ created_at: null, updated_at: '2026-10-12T00:00:00Z' }, now)).toBe(1);
  });
});

describe('change map', () => {
  it('splits lines by area from the paths, the uncovered rest as other', () => {
    expect(
      [
        'src/a.ts',
        'tests/a.rs',
        'ui/x.test.ts',
        'tests/Unit/FooTest.php',
        'config/a.yaml',
        'Dockerfile',
        'README.md',
      ].map(areaOf),
    ).toEqual(['code', 'tests', 'tests', 'tests', 'config', 'config', 'other']);
    const files = [
      { path: 'src/a.ts', additions: 10, deletions: 5 },
      { path: 'tests/a.ts', additions: 20, deletions: 0 },
    ];
    expect(changeMap({ additions: 40, deletions: 10, changed_files: 120 }, files)).toEqual({
      areas: { code: 15, tests: 20, config: 0, other: 15 },
      lines: 50,
      files: 120,
    });
    expect(changeMap({ additions: null, deletions: null, changed_files: null }, [])).toBeNull();
    expect(
      filesByArea([...files, { path: 'src/b.ts', additions: 30, deletions: 0 }]).map((g) => [
        g.area,
        g.files.map((f) => f.path),
      ]),
    ).toEqual([
      ['code', ['src/b.ts', 'src/a.ts']],
      ['tests', ['tests/a.ts']],
    ]);
  });

  it('grows linearly up to 2k lines, then cuts', () => {
    expect(barOf(500)).toEqual({ width: 0.25, capped: false });
    expect(barOf(7000)).toEqual({ width: 1, capped: true });
    expect([kLines(85), kLines(1300), kLines(7000)]).toEqual(['85', '1.3k', '7k']);
  });
});

describe('beforeMerging', () => {
  it('lifts the section a heading names out of the description', () => {
    const body =
      '<p>Intro</p><h2>Before merging</h2><ul><li>Run the backfill</li></ul><h2>Notes</h2><p>x</p>';
    expect(beforeMerging(body)).toEqual({
      before: '<ul><li>Run the backfill</li></ul>',
      rest: '<p>Intro</p><h2>Notes</h2><p>x</p>',
    });
    expect(beforeMerging('<p><strong>Deploy</strong></p><p>Migrate first</p>').before).toBe(
      '<p>Migrate first</p>',
    );
    expect(beforeMerging('<p>Nothing</p>')).toEqual({ before: null, rest: '<p>Nothing</p>' });
  });
});

describe('ownStep', () => {
  it("words my PR's next step as the Queue does", () => {
    const rv: Parameters<typeof ownStep>[0] = { decision: null, ci: 'success', waiting_on: [] };
    expect(ownStep({ ...rv, decision: 'changes_requested' })).toBe('Fix with Claude');
    expect(ownStep({ ...rv, ci: 'failure' })).toBe('Send failures to Claude');
    expect(ownStep({ ...rv, waiting_on: ['bob', 'carol'] })).toBe('Nudge bob');
    expect(ownStep(rv)).toBe('');
  });
});
