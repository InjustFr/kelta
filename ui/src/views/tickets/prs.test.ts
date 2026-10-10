import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { PrLink } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';

import { openContent } from '../work/nav';
import { ciLamp, mainPr, openPr, openPrs, prLabel, prLamp, reviewWord } from './prs';

vi.mock('../work/nav', () => ({ openContent: vi.fn(async () => true) }));

let mock: MockControls;
beforeEach(() => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  vi.mocked(openContent).mockClear();
});

const pr = (over: Partial<PrLink> = {}): PrLink => ({
  url: 'https://github.com/acme/shop-api/pull/7',
  account: 'github-acme',
  repo: 'acme/shop-api',
  number: 7,
  title: 'SHOP-1: x',
  branch: 'feat/x',
  state: 'open',
  draft: false,
  ci: 'success',
  review: null,
  source: 'key_match',
  ...over,
});

describe('PR links', () => {
  it('labels MRs with !, maps CI to lamp shapes and words the review state', () => {
    expect(prLabel(pr())).toBe('#7');
    expect(prLabel(pr({ url: 'https://gitlab.example/g/p/-/merge_requests/7' }))).toBe('!7');
    expect([ciLamp('success'), ciLamp('pending'), ciLamp('failure'), ciLamp('none')]).toEqual([
      'done',
      'working',
      'error',
      'none',
    ]);
    expect(reviewWord(pr({ state: 'merged' }))).toBe('Merged');
    expect(reviewWord(pr({ draft: true }))).toBe('Draft');
    expect(reviewWord(pr({ review: 'changes_requested' }))).toBe('Changes requested');
  });

  it('folds CI and review into one lamp', () => {
    expect(prLamp(pr({ review: 'changes_requested', ci: 'success' }))).toBe('needs_input');
    expect(prLamp(pr({ review: 'approved', ci: 'failure' }))).toBe('error');
    expect(prLamp(pr({ review: 'approved', ci: 'pending' }))).toBe('working');
    expect(prLamp(pr({ review: 'approved', ci: 'none' }))).toBe('done');
    expect(prLamp(pr({ review: 'review_required' }))).toBe('activity');
    expect(prLamp(pr())).toBe('done'); // no review policy: CI alone
  });

  it('shows the first open PR, else the first', () => {
    const merged = pr({ number: 1, state: 'merged' });
    expect(mainPr([merged, pr({ number: 2 })])?.number).toBe(2);
    expect(mainPr([merged])?.number).toBe(1);
    expect(mainPr([])).toBeNull();
  });

  it('opens in Kelta when the code host is bound, else in the browser; several PRs ask for the picker', async () => {
    openPr(pr(), 'shop');
    expect(openContent).toHaveBeenCalledWith('shop', {
      kind: 'review_detail',
      review: { account: 'github-acme', repo: 'acme/shop-api', number: 7 },
    });
    openPr(pr({ account: null }), 'shop');
    expect(openPrs('SHOP-1', [pr()], 'shop', true)).toBe(false);
    await vi.waitFor(() => expect(mock.calls.filter((c) => c.cmd === 'open_external')).toHaveLength(2));
    expect(openContent).toHaveBeenCalledTimes(1);
    expect(openPrs('SHOP-1', [pr(), pr({ number: 8 })], 'shop', false)).toBe(true);
    expect(openContent).toHaveBeenCalledTimes(1);
  });
});
