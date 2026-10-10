import { describe, expect, it } from 'vitest';

import type { Review } from '$lib/gen';
import * as samples from '$lib/gen/fixtures';

import { nudgedRecently, pingBody, waitingChip } from './nudge';

const now = Date.parse('2026-10-10T12:00:00Z');
const pr = (patch: Partial<Review>): Review => ({
  ...samples.reviewDetail.review,
  kind: 'authored',
  ...patch,
});

describe('nudge', () => {
  it('waiting chip: hours since the oldest wait, amber past the SLA', () => {
    const r = pr({ waiting_on: ['anna'], requested_at: '2026-10-09T10:00:00Z' });
    expect(waitingChip(r, 24, now)).toEqual({ label: 'waiting on @anna · 26h', late: true });
    expect(waitingChip(r, 48, now)?.late).toBe(false);
    expect(waitingChip(pr({ waiting_on: [] }), 24, now)).toBeNull();
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
