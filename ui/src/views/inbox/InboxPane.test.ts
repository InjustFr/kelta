import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { findContent } from '$lib/layout';

import { globalContent, loadGlobalLayout, persistPaneContent } from '../work/nav';
import type { Settings } from '$lib/gen';
import {
  GLOBAL,
  layout,
  projects,
  reviews,
  sessions,
  settings,
  tickets,
  toasts,
  ui,
  work,
} from '$lib/stores';

import { prompts } from '../../shell/confirm.svelte';
import '../work/actions';
import { batch, claudeSlots, startsNow } from '../work/batch.svelte';
import InboxPane from './InboxPane.svelte';
import { focusedSessionId } from '../../shell/nav';
import { hud } from '../../shell/hud.svelte';
import { jumpQueue, rowKey, type NowRow } from './groups';
import { currentSections, nextWaiting, nowSummary, refreshNow } from './now';

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
  // Now lists my tickets only: SHOP-155 (a work item's ticket) must be mine for its title to resolve.
  const flaky = mock.state.tickets.find((t) => t.ticket.ref.key === 'SHOP-155');
  if (flaky) flaky.ticket.assignee = { id: 'u-ada', name: 'Ada Lovelace', login: 'ada', avatar_url: null };
  await Promise.all([projects.load(), sessions.load(), work.load()]);
  ui.view = 'now';
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
      // The mock's one unseen ticket (#145).
      'new',
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
    expect(ui.view).toBeNull();
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

  it('N opens New work item, P parks the selected work row', async () => {
    ui.sheets = [];
    const { container } = mountNow();
    await screen.findByText('Flaky test in cart service');
    const pane = screen.getByTestId('inbox-pane');
    await fireEvent.keyDown(pane, { key: 'N' });
    expect(ui.sheets.map((s) => s.key)).toEqual(['work_new']);
    // The invoice item: Claude waits on me, its editor is live, so it can be parked.
    const id = '0199a6b2-0000-7000-8000-00000000a002';
    const current = () =>
      container.querySelector('.row[aria-current="true"]')?.getAttribute('data-row') ?? '';
    for (let i = 0; i < 10 && !current().includes(id); i++) await fireEvent.keyDown(pane, { key: 'j' });
    expect(current()).toContain(id);
    await fireEvent.keyDown(pane, { key: 'P' });
    await waitFor(() => expect(mock.calls.find((c) => c.cmd === 'work_park')?.args).toEqual({ id }));
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
    ui.view = 'now';
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
    await waitFor(() => expect(ui.view).toBeNull());
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

  it('loads my tickets across every source, and Show all opens the Tickets view on Mine', async () => {
    const mine = mock.state.tickets.find((t) => t.ticket.assignee?.id === 'u-ada');
    if (!mine) throw new Error('fixture missing');
    for (let i = 0; i < 12; i += 1) {
      mock.state.tickets.push({
        ...mine,
        ticket: { ...mine.ticket, ref: { ...mine.ticket.ref, key: `MINE-${i}`, id: `m${i}` } },
      });
      // Seen: unseen ones would list under New for you, not Up next.
      mock.state.nextUp.seen.push(`${mine.ticket.ref.account}:MINE-${i}`);
    }
    mountNow();
    await screen.findByText('MINE-0');
    await fireEvent.keyDown(screen.getByTestId('inbox-pane'), { key: 'End' });
    const more = await screen.findByRole('button', { name: /more, show all$/ });
    const lists = mock.calls.filter((c) => c.cmd === 'tracker_list');
    expect(lists.length).toBeGreaterThan(0);
    // Mine for Up next; unassigned only to resolve Next up items (#145).
    expect(lists.every((c) => ['mine', 'unassigned'].includes((c.args as { who: string }).who))).toBe(true);
    await loadGlobalLayout();
    persistPaneContent(GLOBAL, 'tickets', { who: 'anyone', person: 'u-bob' });
    await fireEvent.click(more);
    expect(ui.view).toBe('tickets');
    expect(globalContent('tickets')).toMatchObject({ scope: { kind: 'all' }, who: 'mine', person: null });
  });

  it('Show all opens Tickets on Mine even when the layout failed to load at startup', async () => {
    const mine = mock.state.tickets.find((t) => t.ticket.assignee?.id === 'u-ada');
    if (!mine) throw new Error('fixture missing');
    for (let i = 0; i < 12; i += 1) {
      mock.state.tickets.push({
        ...mine,
        ticket: { ...mine.ticket, ref: { ...mine.ticket.ref, key: `MINE-${i}`, id: `m${i}` } },
      });
      mock.state.nextUp.seen.push(`${mine.ticket.ref.account}:MINE-${i}`);
    }
    mock.failNext('layout_get', { code: 'internal' });
    await expect(loadGlobalLayout()).rejects.toBeTruthy();
    expect(layout.get(GLOBAL)).toBeNull();
    mountNow();
    await screen.findByText('MINE-0');
    await fireEvent.keyDown(screen.getByTestId('inbox-pane'), { key: 'End' });
    await fireEvent.click(await screen.findByRole('button', { name: /more, show all$/ }));
    await waitFor(() => expect(ui.view).toBe('tickets'));
    const tab = layout.get(GLOBAL)?.tabs.find((t) => t.id === 'tickets');
    expect(tab?.root.type === 'pane' && tab.root.content).toMatchObject({ who: 'mine', person: null });
  });

  it('a ticket row says who it waits on instead of the bare assignee', async () => {
    const base = mock.state.tickets.find((t) => t.ticket.assignee?.id === 'u-ada');
    if (!base) throw new Error('fixture missing');
    const ticket = { ...base.ticket, ref: { ...base.ticket.ref, key: 'WAIT-1', id: 'w1' }, priority_rank: 0 };
    const waiting = [
      { person: 'gh:anna', name: 'Anna', why: 'review', since: '2026-01-01T00:00:00Z', matched: true },
    ] as const;
    mock.state.tickets.push({ ...base, ticket, waiting: [...waiting] });
    mock.state.nextUp.seen.push(`${ticket.ref.account}:WAIT-1`);
    const { container } = mountNow();
    await screen.findByText('WAIT-1');
    await nextWaitingTo(container, `t:${ticket.ref.account}:WAIT-1`);
    expect(await screen.findByText(/waiting on Anna \(review\) · \d+h/)).toBeTruthy();
  });

  it('with no ticket source: a notice above the list, the empty state when Now is empty', async () => {
    (mock.state.settings.value as unknown as Settings).sources = [];
    settings.effective = {};
    const { unmount } = mountNow();
    await fireEvent.click(await within(await screen.findByTestId('now-no-source')).findByRole('button'));
    expect(ui.sheet).toMatchObject({ key: 'tracker.source_picker', props: {} });
    unmount();
    ui.sheets = [];
    mock.state.tickets = [];
    mock.state.reviews = [];
    sessions.byId = {};
    work.byId = {};
    mountNow();
    expect(await screen.findByText('Add a ticket source: pick a board')).toBeTruthy();
    expect(screen.queryByTestId('now-no-source')).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Pick a board' }));
    expect(ui.sheet).toMatchObject({ key: 'tracker.source_picker', props: {} });
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

describe('Next up (#145)', () => {
  it('lists my order beside Ready for review; J/K reorder, S starts the top ones by free slots', async () => {
    const keys = ['SHOP-151', 'SHOP-160', '4610'];
    mock.state.nextUp.items = keys.map((key, rank) => {
      const t = mock.state.tickets.find((x) => x.ticket.ref.key === key)!;
      return { project_id: t.project_ids[0]!, ticket: t.ticket.ref, rank, snoozed_until: null };
    });
    const { container } = mountNow();
    const order = () =>
      [...container.querySelectorAll('.row[data-row^="n:"]')].map(
        (e) => e.getAttribute('data-row')!.split(':')[2],
      );
    await waitFor(() => expect(order()).toEqual(keys));
    expect(sectionIds(container).slice(1, 3)).toEqual(['to_review', 'next_up']);
    // Listed tickets leave Up next.
    expect(container.querySelector('[data-row="t:jira-acme:SHOP-151"]')).toBeNull();

    await nextWaitingTo(container, 'n:');
    const pane = screen.getByTestId('inbox-pane');
    await fireEvent.keyDown(pane, { key: 'J' });
    await waitFor(() => expect(order()).toEqual(['SHOP-160', 'SHOP-151', '4610']));
    await fireEvent.keyDown(pane, { key: 'K' });
    await waitFor(() => expect(order()).toEqual(keys));

    await fireEvent.keyDown(pane, { key: 'S' });
    expect(ui.sheet?.key).toBe('start_batch');
    const { live, max } = claudeSlots();
    expect(batch.list.map((m) => m.ref.key)).toEqual(keys.slice(0, startsNow(3, live, max)));
    batch.clear();
    ui.closeSheet();

    // Closed in the tracker: gone from the list on the next refresh, and from the store.
    mock.state.tickets.find((t) => t.ticket.ref.key === 'SHOP-160')!.ticket.status.category = 'done';
    await refreshNow(true);
    await waitFor(() => expect(order()).toEqual(['SHOP-151', '4610']));
    expect(mock.state.nextUp.items.map((i) => i.ticket.key).sort()).toEqual(['4610', 'SHOP-151']);
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
      tickets.load({ kind: 'all' }, null, false, 'mine'),
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
      expect(ui.view).toBeNull();
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
  if (row.type === 'outside') return null;
  return row.ticket.project_ids[0] ?? null;
}
