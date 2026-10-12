import { describe, expect, it } from 'vitest';

import * as samples from '$lib/gen/fixtures';

import { draftBlocked } from './draft';

const authored = { ...samples.reviewDetail.review, kind: 'authored' as const, assigned: false };

describe('draftBlocked', () => {
  it('only lets my own authored PRs through, not ones assigned to me', () => {
    expect(draftBlocked(authored)).toBeNull();
    expect(draftBlocked({ ...authored, assigned: true })).toBe('Only on your own pull requests');
    expect(draftBlocked({ ...authored, kind: 'review_requested' })).toBe('Only on your own pull requests');
  });
});
