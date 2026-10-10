import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { findContent } from '$lib/layout';
import { layout, projects, reviews, sessions, tickets, toasts, ui, work } from '$lib/stores';

import '../work/actions';
import InboxPane from './InboxPane.svelte';
import { nextWaiting, nowSummary } from './now';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  tickets.lists = {};
  reviews.lists = {};
  layout.byProject = {};
  work.git = {};
  toasts.clear();
  await Promise.all([projects.load(), sessions.load(), work.load()]);
  ui.inboxActive = true;
});

function mountNow() {
  return render(InboxPane, {
    props: {
      projectId: 'inbox',
      tabId: 'inbox',
      paneId: 'inbox',
      content: { kind: 'inbox' },
      visible: true,
      focused: true,
    },
  });
}

const sectionIds = (c: HTMLElement) =>
  [...c.querySelectorAll('[data-section]')].map((e) => e.getAttribute('data-section'));

describe('Now', () => {
  it('shows every section in order, with the split header', async () => {
    const { container } = mountNow();
    await waitFor(() => expect(sectionIds(container)).toContain('up_next'));
    expect(sectionIds(container)).toEqual([
      'needs_you',
      'to_review',
      'fix',
      'requests',
      'ship',
      'in_flight',
      'up_next',
    ]);
    const header = screen.getByTestId('now-header');
    expect(header.textContent).toContain('Claude: 1 asks, 1 ready');
    // #311, #98, #7 and #101 (updated since my review).
    expect(header.textContent).toMatch(/Teammates: 4 PRs/);
    expect(header.title).toBe(nowSummary().header);
    // A work item shows once; its sessions are not listed as plain sessions.
    expect(container.querySelectorAll('[data-row^="w:0199a6b2-0000-7000-8000-00000000a004"]')).toHaveLength(
      1,
    );
  });

  it('only the selected row expands, with its actions and keys', async () => {
    const { container } = mountNow();
    await screen.findByText('Flaky test in cart service');
    const pane = screen.getByTestId('inbox-pane');
    // First row: Claude needs you (the scratch item that replied).
    expect(container.querySelector('.row[aria-current="true"]')?.textContent).toContain('wip');
    await fireEvent.keyDown(pane, { key: 'j' });
    const details = screen.getAllByTestId('now-detail');
    expect(details).toHaveLength(1);
    expect(details[0]!.textContent).toContain('Fixed the race in CartServiceTest');
    expect(within(details[0]!).getByText('Review diff')).toBeTruthy();
  });

  it('Enter on To review opens the diff in the work tab and leaves Now (B1)', async () => {
    mountNow();
    await screen.findByText('Flaky test in cart service');
    const pane = screen.getByTestId('inbox-pane');
    await fireEvent.keyDown(pane, { key: 'j' });
    await fireEvent.keyDown(pane, { key: 'Enter' });
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_diff')).toBe(true));
    expect(ui.inboxActive).toBe(false);
    expect(projects.activeId).toBe('shop');
    await waitFor(() => {
      const l = layout.get('shop')!;
      expect(
        findContent(l, (c) => c.kind === 'terminal' && sessions.get(c.session_id)?.name === 'diff'),
      ).toBeTruthy();
    });
  });

  it('x marks reviewed; a blocked key flashes its reason', async () => {
    mountNow();
    await screen.findByText('Flaky test in cart service');
    const pane = screen.getByTestId('inbox-pane');
    await fireEvent.keyDown(pane, { key: 'j' });
    await fireEvent.keyDown(pane, { key: 'c' });
    expect(toasts.list.at(-1)?.toast.text).toMatch(/Only while a rebase is stopped/);
    await fireEvent.keyDown(pane, { key: 'x' });
    await waitFor(() => expect(work.get('0199a6b2-0000-7000-8000-00000000a004')?.review_due).toBe(false));
  });

  it('Enter on a review request opens it in the project Reviews tab, reused (B1)', async () => {
    const { container } = mountNow();
    await waitFor(() => expect(sectionIds(container)).toContain('requests'));
    await nextWaitingTo(container, 'r:');
    await fireEvent.keyDown(screen.getByTestId('inbox-pane'), { key: 'Enter' });
    await waitFor(() => expect(ui.inboxActive).toBe(false));
    const tabs = () => layout.get('shop')!.tabs.filter((t) => t.title === 'Reviews');
    await waitFor(() => expect(tabs()).toHaveLength(1));
  });

  it('filters on id, title, project and branch', async () => {
    const { container } = mountNow();
    await screen.findByText('Flaky test in cart service');
    const pane = screen.getByTestId('inbox-pane');
    await fireEvent.keyDown(pane, { key: '/' });
    const input = await screen.findByLabelText('Filter');
    await fireEvent.input(input, { target: { value: 'retry-queue' } });
    await waitFor(() => expect(container.querySelectorAll('.row[data-row]')).toHaveLength(1));
  });

  it('shows an error state when nothing loads', async () => {
    mock.failAlways('tracker_list', { code: 'needs_auth', message: '401' });
    mock.failAlways('review_list', { code: 'needs_auth', message: '401' });
    work.byId = {};
    mountNow();
    expect(await screen.findByText('Could not load Now')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Re-authenticate' })).toBeTruthy();
  });
});

/** Moves the selection with j until the selected row id starts with `prefix`. */
async function nextWaitingTo(container: HTMLElement, prefix: string): Promise<void> {
  const pane = screen.getByTestId('inbox-pane');
  for (let i = 0; i < 20; i += 1) {
    const id = container.querySelector('.row[aria-current="true"]')?.getAttribute('data-row') ?? '';
    if (id.startsWith(prefix)) return;
    await fireEvent.keyDown(pane, { key: 'j' });
  }
  throw new Error(`no row ${prefix}`);
}

describe('Next waiting', () => {
  it('walks the first four sections in order, cycling', async () => {
    await Promise.all([
      tickets.load({ kind: 'all' }, null),
      reviews.load({ kind: 'all' }, 'review_requested'),
      reviews.load({ kind: 'all' }, 'authored'),
    ]);
    const waiting = nowSummary()
      .sections.filter((s) => ['needs_you', 'to_review', 'fix', 'requests'].includes(s.id))
      .flatMap((s) => s.rows);
    expect(nowSummary().waiting).toBe(waiting.length);
    await nextWaiting(); // the scratch item that replied (billing)
    expect(ui.inboxActive).toBe(false);
    expect(projects.activeId).toBe('billing');
    await nextWaiting(); // SHOP-155 to review
    expect(projects.activeId).toBe('shop');
    for (let i = 2; i < waiting.length; i += 1) await nextWaiting();
    await nextWaiting(); // cycles back to the first
    expect(projects.activeId).toBe('billing');
  });
});
