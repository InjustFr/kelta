import { describe, expect, it } from 'vitest';

import type { Waiting } from '$lib/gen';

import { waitChip } from './waiting';

const now = Date.parse('2026-10-10T12:00:00Z');
const w = (patch: Partial<Waiting>): Waiting => ({
  person: 'p:anna',
  name: 'Anna',
  why: 'review',
  since: '2026-10-09T10:00:00Z',
  matched: true,
  ...patch,
});

describe('waitChip', () => {
  it('one person: name and hours; several: name and a count', () => {
    expect(waitChip([w({})], { now })?.label).toBe('waiting on Anna · 26h');
    expect(waitChip([w({}), w({ name: 'Bob' }), w({ name: 'Cy' })], { now })?.label).toBe(
      'waiting on Anna +2',
    );
    expect(waitChip([], { now })).toBeNull();
  });

  it('a wait on me reads you; an assigned wait has no hours', () => {
    expect(waitChip([w({ person: 'me', name: 'You' })], { now })?.label).toBe('waiting on you · 26h');
    expect(waitChip([w({ why: 'assigned', since: null })], { now })?.label).toBe('waiting on Anna');
  });

  it('names the reason for the author, and everywhere with withWhy', () => {
    expect(waitChip([w({ why: 'author', since: null })], { now })?.label).toBe('waiting on Anna (author)');
    expect(waitChip([w({})], { now, withWhy: true })?.label).toBe('waiting on Anna (review) · 26h');
  });

  it('hover: a line per person with the reason, and the unmatched note', () => {
    const c = waitChip([w({}), w({ name: 'Bob', why: 'author', since: null, matched: false })], { now });
    expect(c?.title).toBe(
      'Anna: review requested 26h ago\nBob: changes requested, author to act. Not matched to a tracker user',
    );
  });

  it('late past the SLA', () => {
    expect(waitChip([w({})], { now, slaHours: 24 })?.late).toBe(true);
    expect(waitChip([w({})], { now, slaHours: 48 })?.late).toBe(false);
    expect(waitChip([w({})], { now })?.late).toBe(false);
  });
});
