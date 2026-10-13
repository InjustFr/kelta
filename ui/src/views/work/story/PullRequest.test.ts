import { render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { WorkItem } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { reviews, work } from '$lib/stores';

import PullRequest from './PullRequest.svelte';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  work.byId = {};
  await Promise.all([work.load(), reviews.load({ kind: 'all' }, 'authored')]);
});

const item = (i: number, over: Partial<WorkItem> = {}): WorkItem =>
  ({ ...mock.state.work[i], ...over }) as WorkItem;
const mount = (w: WorkItem) => render(PullRequest, { props: { item: w, open: true } });
const buttons = () =>
  [...screen.getByTestId('story-pr').querySelectorAll('button')].map((b) => b.textContent!.trim());

describe('Story > Pull request', () => {
  it('SHOP-120 (#305): 3 threads with the first one quoted, reviewers, who it waits on', async () => {
    mount(
      item(6, {
        pr_url: 'https://github.com/acme/shop-api/pull/305',
        state: { kind: 'pr_open', detail: null } as WorkItem['state'],
      }),
    );
    expect((await screen.findByTestId('story-pr-threads')).textContent).toContain('3 unresolved threads');
    await waitFor(() =>
      expect(screen.getByTestId('story-pr').querySelector('blockquote')?.textContent).toContain(
        'Reset the counter',
      ),
    );
    expect(screen.getByTestId('story-pr-reviewers').textContent).toMatch(/Bob\s*approved/);
    expect(screen.getByTestId('story-pr-reviewers').textContent).toContain('waiting on Anna');
    expect(buttons()).toEqual(['Fix with Claude', 'Merge when ready', 'Open PR']);
  });

  it('#12 (PR 13): ready, approved, passing checks, no threads', async () => {
    mount(item(2));
    const checks = await screen.findByTestId('story-pr-checks');
    expect(checks.querySelectorAll('li').length).toBeGreaterThan(0);
    const pr = screen.getByTestId('story-pr');
    expect(pr.textContent).toContain('#13');
    expect(pr.textContent).toContain('Ready for review');
    expect(screen.getByTestId('story-pr-reviewers').textContent).toContain('approved');
    expect(screen.queryByTestId('story-pr-threads')).toBeNull();
    expect(buttons()).toEqual(['Merge when ready', 'Open PR']);
  });

  it('no PR: says what Ship does and offers Ship', async () => {
    mount(item(0)); // SHOP-142
    const pr = await screen.findByTestId('story-pr');
    expect(pr.textContent).toContain('Ship pushes the branch and opens a draft PR.');
    expect(buttons()).toEqual(['Ship']);
  });
});
