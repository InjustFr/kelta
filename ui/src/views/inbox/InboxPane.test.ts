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

const zoneIds = (c: HTMLElement) =>
  [...c.querySelectorAll('[data-zone]')].map((e) => e.getAttribute('data-zone'));
const rowZone = (c: HTMLElement, id: string) => {
  const cls = c.querySelector(`.row[data-row="${id}"]`)?.classList;
  return (['yours', 'claude', 'waiting'] as const).find((z) => cls?.contains(z));
};
const FLAKY = '0199a6b2-0000-7000-8000-00000000a004';
/** SHOP-142, whose Claude is working in the mock. */
const RATE_LIMIT = '0199a6b2-0000-7000-8000-00000000a001';
/** wip/retry-queue-spike, whose Claude has a task list but no changes yet. */
const SPIKE = '0199a6b2-0000-7000-8000-00000000a005';

describe('Now', () => {
  it('one list in three zones, a summary line, no chips, Compact, Refresh or key bar (#399, #413)', async () => {
    const { container } = mountNow();
    await waitFor(() => expect(zoneIds(container)).toEqual(['yours', 'claude', 'waiting']));
    const { yours, claude, others } = nowSummary();
    const header = screen.getByTestId('now-header').textContent;
    expect(header).toContain(`${yours} your move`);
    expect(header).toContain(`${claude} Claude working`);
    expect(header).toContain(`${others} waiting on others`);
    expect(container.querySelector('[data-zone="yours"] .count')?.textContent).toBe(String(yours));
    expect(container.querySelector('[data-zone="claude"] .k-group-hint')?.textContent).toBe(
      'Your parallel capacity: back in minutes.',
    );
    // A diff to review is my move; a Claude at work is Claude working.
    expect(rowZone(container, `w:${FLAKY}`)).toBe('yours');
    expect(rowZone(container, `w:${RATE_LIMIT}`)).toBe('claude');
    expect(screen.queryByRole('switch', { name: 'Compact' })).toBeNull();
    expect(screen.queryByRole('button', { name: /^Refresh/ })).toBeNull();
    expect(container.querySelector('.k-keyhints')).toBeNull();
    // A work item shows once; its sessions are not listed as plain sessions.
    expect(container.querySelectorAll(`[data-row^="w:${FLAKY}"]`)).toHaveLength(1);
  });

  it('every row has a stage track; the selected one names its stage', async () => {
    const { container } = mountNow();
    await screen.findByText('Flaky test in cart service');
    await nextWaitingTo(container, `w:${FLAKY}`);
    const row = container.querySelector<HTMLElement>(`.row[data-row="w:${FLAKY}"]`)!;
    const track = within(row).getByRole('img');
    expect(track.getAttribute('data-mark')).toBe('you');
    expect(track.getAttribute('aria-label')).toMatch(/^At Claude, your move; furthest stage reached: /);
    expect(track.textContent?.trim()).toBe('Claude');
    expect(container.querySelectorAll('.row [data-mark]').length).toBe(
      container.querySelectorAll('.row[data-row]').length,
    );
  });

  it('the brief: primary filled, two next actions, the rest behind "more actions" (.), message, facts', async () => {
    const { container } = mountNow();
    await screen.findByText('Flaky test in cart service');
    await nextWaitingTo(container, `w:${FLAKY}`);
    const pane = screen.getByTestId('inbox-pane');
    // The row: its next step as a button, the next: note and the first line of Claude's message.
    const row = container.querySelector('.row[aria-current="true"]')!;
    expect(within(row as HTMLElement).getByRole('button', { name: 'Review changes' })).toBeTruthy();
    expect(row.textContent).toContain('next: check the retry path against staging');
    expect(row.querySelector('.meta.warn')?.textContent).toBe('tests: none');
    const brief = screen.getByTestId('now-brief');
    const primary = brief.querySelector<HTMLElement>('.btn.primary')!;
    expect(primary.textContent).toMatch(/^Review changes/);
    expect(primary.querySelector('kbd')).not.toBeNull();
    expect(within(brief).getByRole('button', { name: /^\d+ more actions/ })).toBeTruthy();
    const shown = brief.querySelectorAll('.btn:not(.more)').length;
    expect(shown).toBe(3);
    expect(brief.textContent).toContain('212 lines');
    expect(brief.textContent).toContain(
      'I left the backoff constant at 3 tries; say if you want it configurable.',
    );
    await fireEvent.keyDown(pane, { key: '.' });
    expect(within(brief).queryByRole('button', { name: /more actions/ })).toBeNull();
    expect(brief.querySelectorAll('.btn').length).toBeGreaterThan(shown);
    await fireEvent.click(within(brief).getByRole('button', { name: /^Mark reviewed/ }));
    await waitFor(() => expect(work.get(FLAKY)?.review_due).toBe(false));
  });

  it("the brief of a work row shows Claude's tasks and the changes so far; without tasks, nothing (#434)", async () => {
    const { container } = mountNow();
    await screen.findByText('Flaky test in cart service');
    // The retry-queue spike: tasks, nothing changed yet.
    await nextWaitingTo(container, `w:${SPIKE}`);
    const spike = await screen.findByTestId('now-brief-progress');
    expect(spike.querySelector('h4')?.textContent).toMatch(/Tasks\s+2 of 4/);
    expect(spike.querySelector('.files')).toBeNull();
    // No task list: no section.
    await nextWaitingTo(container, `w:${FLAKY}`);
    expect(screen.queryByTestId('now-brief-progress')).toBeNull();
    await nextWaitingTo(container, `w:${RATE_LIMIT}`);
    const progress = await screen.findByTestId('now-brief-progress');
    expect(progress.querySelector('h4')?.textContent).toMatch(/Tasks\s+3 of 5/);
    expect(progress.querySelectorAll('.tasks li')).toHaveLength(5);
    expect(progress.querySelector('li[aria-current="step"]')?.textContent).toContain('Cover the burst case');
    await waitFor(() => expect(progress.textContent).toContain('+41 −7, 3 files'));
    expect(progress.querySelector('.files li')?.textContent).toBe('src/rate_limit.rs');
  });

  it("the row's next step is a button that runs it", async () => {
    const { container } = mountNow();
    await screen.findByText('Flaky test in cart service');
    const row = container.querySelector<HTMLElement>(`.row[data-row="w:${FLAKY}"]`)!;
    await fireEvent.click(within(row).getByRole('button', { name: 'Review changes' }));
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_diff')).toBe(true));
  });

  it('Enter on a clicked (button-focused) row still runs its next action', async () => {
    const { container } = mountNow();
    await screen.findByText('Flaky test in cart service');
    const row = container.querySelector<HTMLElement>(`.row[data-row="w:${FLAKY}"]`)!;
    await fireEvent.click(row);
    row.focus();
    await fireEvent.keyDown(row, { key: 'Enter' });
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_diff')).toBe(true));
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
    await waitFor(() =>
      expect(container.querySelector(`.row[data-row="w:${FLAKY}"] [data-next]`)?.textContent).not.toBe(
        'Review changes',
      ),
    );
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
    await waitFor(() => expect(container.querySelector('.row[data-row^="r:"]')).not.toBeNull());
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
    const input = await screen.findByLabelText('Filter Queue');
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

  it('Waiting to deploy: last, folded until its header opens it, rows show the status (#391)', async () => {
    const base = mock.state.tickets.find((t) => t.ticket.assignee?.id === 'u-ada');
    if (!base) throw new Error('fixture missing');
    const status = { ...base.ticket.status, name: 'A MEPP', category: 'in_progress' as const };
    const ticket = { ...base.ticket, ref: { ...base.ticket.ref, key: 'DEPLOY-1', id: 'd1' }, status };
    mock.state.tickets.push({ ...base, ticket });
    const value = mock.state.settings.value as unknown as Settings;
    value.sources = value.sources.map((s) => ({ ...s, waiting_statuses: ['A MEPP'] }));
    settings.effective = {};
    await settings.load();
    const { container } = mountNow();
    await waitFor(() => expect(container.querySelector('[data-section="waiting"]')).not.toBeNull());
    expect(screen.queryByText('DEPLOY-1')).toBeNull();
    const header = within(container.querySelector<HTMLElement>('[data-section="waiting"]')!).getByRole(
      'button',
      { name: /Waiting to deploy/ },
    );
    expect(header.getAttribute('aria-expanded')).toBe('false');
    // Keys typed on the header must not bubble into row actions (Enter would start the selected ticket).
    const calls = mock.calls.length;
    await fireEvent.keyDown(header, { key: 'Enter' });
    await fireEvent.keyDown(header, { key: 'R' });
    await new Promise((r) => setTimeout(r, 20));
    expect(mock.calls.length).toBe(calls);
    await fireEvent.click(header);
    // The list is virtual: End selects the last row and scrolls to it.
    await fireEvent.keyDown(screen.getByTestId('inbox-pane'), { key: 'End' });
    const row = await waitFor(() => {
      const el = container.querySelector<HTMLElement>('.row[data-row$=":DEPLOY-1"]');
      expect(el).not.toBeNull();
      return el!;
    });
    expect(within(row).getByText('A MEPP')).toBeTruthy();
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
    expect(await screen.findByText('Could not load the Queue')).toBeTruthy();
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
    expect(rowZone(container, 'n:jira-acme:SHOP-151')).toBe('yours');
    // Listed tickets leave Up next.
    expect(container.querySelector('[data-row="t:jira-acme:SHOP-151"]')).toBeNull();

    await nextWaitingTo(container, 'n:');
    const pane = screen.getByTestId('inbox-pane');
    await fireEvent.keyDown(pane, { key: 'J' });
    await waitFor(() => expect(order()).toEqual(['SHOP-160', 'SHOP-151', '4610']));
    await fireEvent.keyDown(pane, { key: 'K' });
    await waitFor(() => expect(order()).toEqual(keys));
    // #397: the mouse path, a drag of the first ticket onto the last.
    const at = (key: string) =>
      container.querySelector<HTMLElement>(`.row[data-row$=":${key}"][data-row^="n:"]`)!;
    expect(at('SHOP-151').getAttribute('draggable')).toBe('true');
    await fireEvent.dragStart(at('SHOP-151'));
    await fireEvent.dragOver(at('4610'));
    await fireEvent.drop(at('4610'));
    await waitFor(() => expect(order()).toEqual(['SHOP-160', '4610', 'SHOP-151']));
    await fireEvent.dragStart(at('SHOP-151'));
    await fireEvent.drop(at('SHOP-160'));
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
