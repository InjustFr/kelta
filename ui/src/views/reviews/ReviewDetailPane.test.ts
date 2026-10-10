import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { ReviewRef } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { findContent } from '$lib/layout';
import { layout, reviews, toasts } from '$lib/stores';

import ReviewDetailPane from './ReviewDetailPane.svelte';

let mock: MockControls;
const ref: ReviewRef = { account: 'github-acme', repo: 'acme/shop-api', number: 311 };

beforeEach(() => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  reviews.details = {};
  layout.byProject = {};
  toasts.clear();
});

function mountDetail() {
  return render(ReviewDetailPane, {
    props: {
      projectId: 'shop',
      tabId: 't',
      paneId: 'p',
      content: { kind: 'review_detail', review: ref },
      visible: true,
      focused: true,
    },
  });
}

const approveCalls = () => mock.calls.filter((c) => c.cmd === 'review_approve');

describe('ReviewDetailPane', () => {
  it('opens a linked ticket from its badge', async () => {
    mock.state.reviews.find((r) => r.review.ref.number === 311)!.review.linked_tickets = ['SHOP-120'];
    mountDetail();
    await fireEvent.click(await screen.findByRole('button', { name: 'Open SHOP-120' }));
    await waitFor(() => {
      const l = layout.get('shop');
      expect(l && findContent(l, (c) => c.kind === 'ticket_detail')).toBeTruthy();
    });
  });

  it('approves with the head_sha that is displayed', async () => {
    mountDetail();
    const sha = await screen.findByTestId('head-sha');
    const shown = sha.getAttribute('title') as string;
    expect(shown.length).toBeGreaterThan(7);
    await fireEvent.click(screen.getByRole('button', { name: 'Approve' }));
    await waitFor(() => expect(approveCalls()).toHaveLength(1));
    expect(approveCalls()[0]?.args).toEqual({ review: ref, head_sha: shown });
    await waitFor(() => expect(toasts.list.some((t) => t.toast.text.startsWith('Approved'))).toBe(true));
  });

  it('keeps sending the displayed sha when the PR moved, then shows "PR changed, refresh"', async () => {
    mountDetail();
    const sha = await screen.findByTestId('head-sha');
    const shown = sha.getAttribute('title') as string;
    const item = mock.state.reviews.find((r) => r.review.ref.number === 311);
    if (!item) throw new Error('fixture missing');
    item.review.head_sha = 'f'.repeat(40); // someone pushed after the page loaded

    await fireEvent.keyDown(screen.getByTestId('review-detail'), { key: 'a' });
    await waitFor(() => expect(approveCalls()).toHaveLength(1));
    expect((approveCalls()[0]?.args as { head_sha: string }).head_sha).toBe(shown);
    expect(await screen.findByTestId('pr-changed')).toBeTruthy();
    expect(toasts.list.some((t) => t.toast.text === 'PR changed, refresh')).toBe(true);

    await fireEvent.click(within(screen.getByTestId('pr-changed')).getByRole('button', { name: 'Refresh' }));
    await waitFor(() => expect(screen.getByTestId('head-sha').getAttribute('title')).toBe('f'.repeat(40)));
    expect(screen.queryByTestId('pr-changed')).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Approve' }));
    await waitFor(() => expect(approveCalls()).toHaveLength(2));
    expect((approveCalls()[1]?.args as { head_sha: string }).head_sha).toBe('f'.repeat(40));
  });

  it('comments and requests changes with a body', async () => {
    mountDetail();
    await screen.findByTestId('head-sha');
    await fireEvent.click(screen.getByRole('button', { name: 'Request changes' }));
    const body = await screen.findByLabelText(/What should change/);
    const submit = screen.getAllByRole('button', { name: 'Request changes' }).at(-1) as HTMLElement;
    expect(submit.hasAttribute('disabled')).toBe(true);
    await fireEvent.input(body, { target: { value: 'Please add tests' } });
    await fireEvent.click(submit);
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'review_request_changes')).toBe(true));
    expect(mock.calls.filter((c) => c.cmd === 'review_request_changes').at(-1)?.args).toEqual({
      review: ref,
      body: 'Please add tests',
    });

    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    await fireEvent.keyDown(screen.getByTestId('review-detail'), { key: 'm' });
    const comment = await screen.findByLabelText(/Comment \(Markdown\)/);
    await fireEvent.input(comment, { target: { value: 'LGTM so far' } });
    await fireEvent.keyDown(comment, { key: 'Enter', ctrlKey: true });
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'review_comment')).toBe(true));
  });

  it('shows pending comments and lets a decision go out without a body', async () => {
    mock.state.pending['acme/shop-api#311'] = 2;
    mountDetail();
    expect((await screen.findByTestId('pending-comments')).textContent).toContain('2 pending comments');
    await fireEvent.keyDown(screen.getByTestId('review-detail'), { key: 'c' });
    await screen.findByLabelText(/What should change/);
    const submit = screen.getAllByRole('button', { name: 'Request changes' }).at(-1) as HTMLElement;
    expect(submit.hasAttribute('disabled')).toBe(false);
    await fireEvent.click(submit);
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'review_request_changes')).toBe(true));
    await waitFor(() => expect(screen.queryByTestId('pending-comments')).toBeNull());
  });

  it('opens the review-locally flow from the keyboard', async () => {
    mountDetail();
    await screen.findByTestId('head-sha');
    await fireEvent.keyDown(screen.getByTestId('review-detail'), { key: 's' });
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_plan')).toBe(true));
    expect(mock.calls.filter((c) => c.cmd === 'work_plan').at(-1)?.args).toMatchObject({
      source: { kind: 'review', review: ref },
    });
  });

  it('shows an error state with retry when the review cannot be loaded', async () => {
    mock.failAlways('review_get', { code: 'upstream', message: 'GitHub is down' });
    mountDetail();
    expect(await screen.findByText('Could not load the review')).toBeTruthy();
    mock.clearFailures();
    await fireEvent.click(screen.getByRole('button', { name: /retry/i }));
    expect(await screen.findByTestId('head-sha')).toBeTruthy();
  });
});
