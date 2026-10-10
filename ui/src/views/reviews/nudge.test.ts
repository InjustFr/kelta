import { describe, expect, it } from 'vitest';

import type { Review, ReviewItem, Waiting } from '$lib/gen';
import * as samples from '$lib/gen/fixtures';

import { nudgedRecently, pingBody, waitingChip } from './nudge';

const now = Date.parse('2026-10-10T12:00:00Z');
const pr = (patch: Partial<Review>): Review => ({
  ...samples.reviewDetail.review,
  kind: 'authored',
  ...patch,
});

describe('nudge', () => {
  const anna: Waiting = {
    person: 'p:anna',
    name: 'Anna',
    why: 'review',
    since: '2026-10-09T10:00:00Z',
    matched: true,
  };
  const item = (patch: Partial<Review>, waiting: Waiting[]): ReviewItem => ({
    review: pr(patch),
    project_ids: [],
    waiting,
  });

  it('waiting chip: hours since the wait, amber past the SLA on my PRs only', () => {
    const mine = item({}, [anna]);
    expect(waitingChip(mine, 24, now)).toMatchObject({ label: 'waiting on Anna · 26h', late: true });
    expect(waitingChip(mine, 48, now)?.late).toBe(false);
    expect(waitingChip(item({}, []), 24, now)).toBeNull();
    expect(waitingChip(item({ kind: 'review_requested' }, [anna]), 24, now)?.late).toBe(false);
  });

  it('request rows name the author when changes were requested', () => {
    const bob: Waiting = { person: 'p:bob', name: 'Bob', why: 'author', since: null, matched: true };
    expect(waitingChip(item({ kind: 'review_requested' }, [bob]), 24, now)?.label).toBe(
      'waiting on Bob (author)',
    );
  });

  it('cooldown: 24 h after a nudge', () => {
    expect(nudgedRecently(pr({ nudged_at: '2026-10-09T13:00:00Z' }), now)).toBe(true);
    expect(nudgedRecently(pr({ nudged_at: '2026-10-09T11:00:00Z' }), now)).toBe(false);
    expect(nudgedRecently(pr({ nudged_at: null }), now)).toBe(false);
  });

  it('comment ping fills {reviewers}', () => {
    expect(pingBody('{reviewers} ping', pr({ waiting_on: ['anna', 'bob'] }))).toBe('@anna @bob ping');
  });
});
