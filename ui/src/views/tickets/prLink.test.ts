import { describe, expect, it } from 'vitest';

import type { Review, Ticket } from '$lib/gen';
import * as samples from '$lib/gen/fixtures';

import { ciLamp, prForTicket, prLabel } from './prLink';

const ticket = (key: string): Ticket => ({ ...samples.ticket, ref: { ...samples.ticket.ref, key } });

const review = (number: number, linked: string[], patch: Partial<Review> = {}): Review => ({
  ...samples.reviewDetail.review,
  ref: { account: 'gh', repo: 'acme/shop', number },
  url: `https://github.com/acme/shop/pull/${number}`,
  linked_tickets: linked,
  ...patch,
});

describe('prForTicket', () => {
  const reviews = [review(1, ['SHOP-1']), review(2, ['SHOP-2']), review(3, ['#12'])];

  it('prefers the work item PR url', () => {
    expect(prForTicket(ticket('SHOP-1'), 'https://github.com/acme/shop/pull/2', reviews)?.ref.number).toBe(2);
  });

  it('falls back to linked_tickets when the url is not loaded', () => {
    expect(prForTicket(ticket('SHOP-1'), 'https://nowhere/pull/9', reviews)?.ref.number).toBe(1);
    expect(prForTicket(ticket('SHOP-9'), null, reviews)).toBeNull();
  });

  it('matches forge and Redmine keys against #N links in the PR repo, ignoring case', () => {
    expect(prForTicket(ticket('acme/shop#12'), null, reviews)?.ref.number).toBe(3);
    expect(prForTicket(ticket('other/repo#12'), null, reviews)).toBeNull();
    expect(prForTicket(ticket('12'), null, reviews)?.ref.number).toBe(3);
    expect(prForTicket(ticket('shop-2'), null, reviews)?.ref.number).toBe(2);
  });
});

describe('prLabel and ciLamp', () => {
  it('uses ! for merge requests and maps CI to lamp shapes', () => {
    expect(prLabel(review(5, []))).toBe('#5');
    expect(prLabel(review(7, [], { url: 'https://gitlab.example/g/p/-/merge_requests/7' }))).toBe('!7');
    expect([ciLamp('success'), ciLamp('pending'), ciLamp('failure'), ciLamp('none')]).toEqual([
      'done',
      'working',
      'error',
      'none',
    ]);
  });
});
