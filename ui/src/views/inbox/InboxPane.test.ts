import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { findContent } from '$lib/layout';
import { layout, projects, reviews, sessions, tickets, toasts, ui, work } from '$lib/stores';

import { prompts } from '../../shell/confirm.svelte';
import '../work/actions';
import InboxPane from './InboxPane.svelte';
import { focusedSessionId } from '../../shell/nav';
import { hud } from '../../shell/hud.svelte';
import { jumpQueue, rowKey, type NowRow } from './groups';
import { currentSections, nextWaiting, nowSummary } from './now';

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
    expect(header.textContent).toContain('Claude: 1 asks');
    expect(header.textContent).toContain('1 LLM diff');
    // #311, #98, #7 and #101 (updated since my review).
    expect(header.textContent).toMatch(/4 PRs waiting/);
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
    // Ready for review: the next: note and the first line of Claude's full message, the delta chip.
    expect(details[0]!.textContent).toContain('next: check the retry path against staging');
    expect(details[0]!.textContent).toContain('Added the retry wrapper');
    expect(within(details[0]!).getByText('Review changes')).toBeTruthy();
    const row = container.querySelector('.row[aria-current="true"]')!;
    expect(row.textContent).toContain('212 lines');
    expect(row.querySelector('.meta.warn')?.textContent).toBe('tests: none');
    // m unfolds the whole message.
    await fireEvent.keyDown(pane, { key: 'm' });
    const lines = screen.getAllByTestId('now-message').map((l) => l.textContent);
    expect(lines).toContain('I left the backoff constant at 3 tries; say if you want it configurable.');
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

  it('R marks reviewed and the row leaves Ready for review; a blocked key flashes its reason', async () => {
    const { container } = mountNow();
    await screen.findByText('Flaky test in cart service');
    const pane = screen.getByTestId('inbox-pane');
    await fireEvent.keyDown(pane, { key: 'j' });
    await fireEvent.keyDown(pane, { key: 'c' });
    expect(toasts.list.at(-1)?.toast.text).toMatch(/Only while a rebase is stopped/);
    await fireEvent.keyDown(pane, { key: 'R' });
    await waitFor(() => expect(work.get('0199a6b2-0000-7000-8000-00000000a004')?.review_due).toBe(false));
    await waitFor(() => expect(sectionIds(container)).not.toContain('to_review'));
  });

  it('v opens the delta since the last look, V the whole diff, b edits the next: note', async () => {
    mountNow();
    await screen.findByText('Flaky test in cart service');
    const pane = screen.getByTestId('inbox-pane');
    await fireEvent.keyDown(pane, { key: 'j' });
    await fireEvent.keyDown(pane, { key: 'v' });
    const diffs = () =>
      mock.calls.filter((c) => c.cmd === 'work_diff').map((c) => c.args as { delta?: boolean });
    await waitFor(() => expect(diffs()[0]?.delta).toBe(true));
    ui.inboxActive = true;
    await fireEvent.keyDown(pane, { key: 'V' });
    await waitFor(() => expect(diffs()).toHaveLength(2));
    expect(diffs()[1]!.delta).toBeUndefined();
    await fireEvent.keyDown(pane, { key: 'b' });
    await waitFor(() => expect(prompts.current?.value).toBe('check the retry path against staging'));
    prompts.answer('ship it');
    await waitFor(() => expect(work.get('0199a6b2-0000-7000-8000-00000000a004')?.next_note).toBe('ship it'));
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
    const input = await screen.findByLabelText('Filter Now');
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

describe('Next waiting (Mod+J)', () => {
  it('walks the jump queue across projects, focusing the row and counting in the HUD, cycling', async () => {
    await Promise.all([
      tickets.load({ kind: 'all' }, null),
      reviews.load({ kind: 'all' }, 'review_requested'),
      reviews.load({ kind: 'all' }, 'authored'),
    ]);
    const queue = jumpQueue(currentSections(), sessions.all);
    expect(queue.length).toBeGreaterThan(2);
    expect(new Set(queue.map((j) => rowProject(j.row))).size).toBeGreaterThan(1);
    for (const [i, j] of queue.entries()) {
      const before = projects.activeId; // a row without a project opens in the active one
      await nextWaiting();
      expect(hud.text).toBe(`${i + 1}/${queue.length} · ${j.label} · ${rowKey(j.row)}`);
      expect(ui.inboxActive).toBe(false);
      expect(projects.activeId).toBe(rowProject(j.row) ?? before);
      if (j.row.type === 'session') expect(focusedSessionId()).toBe(j.row.session.id);
    }
    await nextWaiting(); // cycles back to the first
    expect(hud.text.startsWith(`1/${queue.length} · `)).toBe(true);
    await nextWaiting(-1); // Mod+Shift+J: the last
    expect(hud.text.startsWith(`${queue.length}/${queue.length} · `)).toBe(true);
  });

  it('offers Tickets when nothing is waiting', async () => {
    sessions.byId = {};
    work.byId = {};
    await nextWaiting();
    expect(hud.text).toBe('nothing waiting');
    expect(hud.enter).toEqual({ action: 'tickets.open', label: 'Tickets' });
  });
});

/** The project a queue row lands in. */
function rowProject(row: NowRow): string | null {
  if (row.type === 'work') return row.item.project_id;
  if (row.type === 'session') return row.session.project_id;
  if (row.type === 'review') return row.review.project_ids[0] ?? null;
  return row.ticket.project_ids[0] ?? null;
}
