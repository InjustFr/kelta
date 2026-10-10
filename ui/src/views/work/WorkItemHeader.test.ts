import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import { sheetRegistry, type RegisteredSheetKey } from '$app/registry';
import type { WorkItem } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { findContent } from '$lib/layout';
import { layout, projects, sessions, tickets, toasts, ui, work } from '$lib/stores';

import { workUi } from './ui.svelte';
import WorkItemHeader from './WorkItemHeader.svelte';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  tickets.details = {};
  tickets.transitions = {};
  toasts.clear();
  work.byId = {};
  work.git = {};
  layout.byProject = {};
  ui.sheets = [];
  Object.assign(workUi, { menu: null });
  // ProjectRail reads git status at startup; the header itself never asks.
  await Promise.all([work.load(), sessions.load(), projects.load(), work.refreshStatus()]);
});

/** Renders the sheet the last action opened (the shell's SheetHost does this in the app). */
async function mountSheet(key: RegisteredSheetKey) {
  await waitFor(() => expect(ui.sheet?.key).toBe(key));
  const Sheet = (await sheetRegistry[key]()).default;
  return render(Sheet, { props: { ...ui.sheet!.props, onclose: () => ui.closeSheet(key) } });
}

function mountHeader(item: WorkItem) {
  return render(WorkItemHeader, { props: { projectId: item.project_id, tabId: 'tab', workItemId: item.id } });
}

const item = (i: number): WorkItem => mock.state.work[i] as WorkItem;

async function openMenu(): Promise<HTMLElement> {
  await fireEvent.click(await screen.findByTestId('work-menu-button'));
  return screen.findByRole('menu', { name: 'Work' });
}

describe('work bar', () => {
  it('shows the phase, branch, ahead/behind and diffstat from work_status_all', async () => {
    const w = item(0);
    mountHeader(w);
    expect(await screen.findByText(w.branch)).toBeTruthy();
    await waitFor(() => expect(screen.getByTestId('ahead').textContent).toBe('↑2'));
    expect(screen.getByTestId('behind').textContent).toBe('↓1');
    expect(screen.getByText('+41')).toBeTruthy();
    expect(screen.getByText('Uncommitted changes')).toBeTruthy();
    expect(screen.getByTestId('work-header').dataset.phase).toBe('working'); // Claude is working
    expect(mock.calls.some((c) => c.cmd === 'work_status_all')).toBe(true);
    expect(mock.calls.some((c) => c.cmd === 'work_status')).toBe(false);
    await waitFor(() => expect(screen.getByRole('button', { name: /In progress/i })).toBeTruthy());
  });

  it('re-reads git status when Claude stops (from the store)', async () => {
    const w = item(0);
    mountHeader(w);
    await waitFor(() => expect(screen.getByTestId('ahead')).toBeTruthy());
    const before = mock.calls.filter((c) => c.cmd === 'work_status_all').length;
    work.upsert({ ...w, review_due: true });
    await waitFor(() =>
      expect(mock.calls.filter((c) => c.cmd === 'work_status_all').length).toBeGreaterThan(before),
    );
  });

  it('To review: the primary opens the delta zoomed in the work tab, R marks reviewed', async () => {
    const w = item(3); // SHOP-155, review_due
    mountHeader(w);
    await waitFor(() => expect(screen.getByTestId('work-header').dataset.phase).toBe('to_review'));
    await fireEvent.click(screen.getByRole('button', { name: 'Review changes' }));
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_diff')).toBe(true));
    // No work tab for SHOP-155 in the fixtures: work_resume recreates it, the diff opens beside it.
    await waitFor(() =>
      expect(
        findContent(
          layout.get('shop')!,
          (c) => c.kind === 'terminal' && sessions.get(c.session_id)?.name === 'diff',
        ),
      ).toBeTruthy(),
    );
    const menu = await openMenu();
    await fireEvent.keyDown(menu, { key: 'R' });
    await waitFor(() => expect(work.get(w.id)?.review_due).toBe(false));
  });

  it('opens WorkItemPane as a split down from the phase label', async () => {
    const w = item(0);
    await layout.ensure('shop');
    mountHeader(w);
    await fireEvent.click(await screen.findByTestId('work-phase'));
    await waitFor(() => expect(findContent(layout.get('shop')!, (c) => c.kind === 'work_item')).toBeTruthy());
  });

  it('work menu: every letter, letters never move, disabled entries say why', async () => {
    mountHeader(item(0));
    const menu = await openMenu();
    const entries = within(menu).getAllByRole('menuitem');
    const label = (e: HTMLElement) => e.querySelector('.label')?.textContent ?? '';
    const labels = entries.map(label);
    expect(labels.length).toBe(18); // primary + v ⇧V p ⇧R f r c a n s b g l t o ⇧M ⇧F
    const cont = entries.find((e) => label(e).startsWith('Continue rebase'))!;
    expect(cont.getAttribute('aria-disabled')).toBe('true');
    expect(cont.title).toBe('Only while a rebase is stopped');
    const mark = entries.find((e) => label(e).startsWith('Mark reviewed'))!;
    expect(mark.title).toBe('Nothing to review');
    await fireEvent.keyDown(menu, { key: 'c' }); // disabled: nothing runs, the menu stays
    expect(screen.getByRole('menu', { name: 'Work' })).toBeTruthy();
  });

  it('review checkouts are read-only: p r f l ⇧M are gated', async () => {
    const r: WorkItem = {
      ...item(0),
      id: 'review-item',
      kind: 'review',
      ticket: null,
      review: { account: 'github-acme', repo: 'acme/shop-api', number: 311 },
    };
    work.upsert(r);
    mountHeader(r);
    const menu = await openMenu();
    const reasons = within(menu)
      .getAllByRole('menuitem')
      .filter((e) => e.title === 'Review checkout: read-only')
      .map((e) => e.querySelector('.label')?.textContent?.split(' ')[0]);
    expect(reasons).toEqual(['Ship', 'Fix', 'Rebase', 'Link', 'Merge']);
  });

  it('p ships through the Ship dialog', async () => {
    const w = { ...item(0) };
    mock.state.sessions.find((s) => s.id === w.session_ids[0])!.status = 'done';
    await sessions.load();
    mountHeader(w);
    const menu = await openMenu();
    await fireEvent.keyDown(menu, { key: 'p' });
    await mountSheet('ship');
    const dialog = await screen.findByRole('dialog');
    const title = within(dialog).getByLabelText('Title') as HTMLInputElement;
    await waitFor(() => expect(title.value).toBe('SHOP-142: Rate-limit login attempts'));
    await fireEvent.input(title, { target: { value: 'My PR' } });
    // The fixture worktree is dirty: Ship anyway.
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Ship anyway' }));
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_create_pr')).toBe(true));
    expect(mock.calls.filter((c) => c.cmd === 'work_create_pr').at(-1)?.args).toMatchObject({
      id: w.id,
      draft: { title: 'My PR', draft: false },
    });
    // PR open with unpushed commits: the next action is Push (another lane's).
    await waitFor(() => expect(screen.getByTestId('work-header').dataset.phase).toBe('unpushed'));
  });

  it('Shift+F finishes: lists the dirty files and only removes with Force', async () => {
    const w = item(1);
    mountHeader(w);
    const menu = await openMenu();
    await fireEvent.keyDown(menu, { key: 'F' });
    await mountSheet('finish');
    const dialog = await screen.findByRole('dialog');
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Finish' }));
    const dirty = await screen.findByTestId('finish-dirty');
    expect(dirty.textContent).toContain('src/invoice.rs');
    await fireEvent.click(screen.getByRole('button', { name: 'Force remove' }));
    await waitFor(() => expect(mock.calls.filter((c) => c.cmd === 'work_finish')).toHaveLength(2));
    expect(mock.calls.filter((c) => c.cmd === 'work_finish').at(-1)?.args).toMatchObject({
      opts: { force: true },
    });
    await waitFor(() => expect(work.get(w.id)?.state.kind).toBe('finished'));
  });

  it('a failed start: the primary retries the step', async () => {
    const w = item(1);
    expect(w.state.kind).toBe('failed');
    mountHeader(w);
    await fireEvent.click(await screen.findByRole('button', { name: /^Retry / }));
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_retry_step')).toBe(true));
  });

  it('rebase: stopped on conflicts → Continue → Rebased → Force push… (confirmed, lease)', async () => {
    const w = item(2); // PR open, Claude idle
    mountHeader(w);
    await fireEvent.click(within(await openMenu()).getByRole('menuitem', { name: /Rebase onto main/ }));
    await waitFor(() => expect(screen.getByTestId('work-header').dataset.phase).toBe('rebase_stopped'));
    expect(screen.getByRole('button', { name: 'Ask Claude to resolve' })).toBeTruthy();
    await fireEvent.click(within(await openMenu()).getByRole('menuitem', { name: /Continue rebase/ }));
    await waitFor(() => expect(screen.getByTestId('work-header').dataset.phase).toBe('rebased'));
    await fireEvent.click(screen.getByRole('button', { name: 'Force push…' }));
    // The dialog is a sheet entry: it is mounted by the sheet host, here we assert what it asks.
    const { ui } = await import('$lib/stores');
    expect(ui.sheet?.key).toBe('work_dialog');
    expect(ui.sheet?.props).toMatchObject({ tone: 'danger', title: 'Force push' });
    expect(String(ui.sheet?.props.text)).toMatch(
      /Rewrites feat\/gh-12-json-output on origin \(#13\)\. The lease checks origin is still at \w{7}\./,
    );
    expect(mock.calls.some((c) => c.cmd === 'work_push')).toBe(false);
    ui.closeSheet();
  });

  it('remote commits get "Rebase onto origin/<branch>", never Force push', async () => {
    const w = item(2); // a reviewer's suggestion commit landed on the remote branch
    mock.state.remoteNew[w.id] = 2;
    await work.refreshStatus();
    const claude = mock.state.sessions.find((x) => w.session_ids.includes(x.id) && x.kind.type === 'claude')!;
    claude.status = 'done'; // it was asking for permission: rebase would be refused
    mountHeader(w);
    await waitFor(() => expect(screen.getByTestId('work-header').dataset.phase).toBe('remote_new'));
    expect(screen.queryByRole('button', { name: 'Force push…' })).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: `Rebase onto origin/${w.branch}` }));
    // The rebase onto the remote branch is followed by the normal rebase onto base (FLOW §4.4).
    await waitFor(() =>
      expect(
        mock.calls
          .filter((c) => c.cmd === 'work_rebase')
          .map((c) => (c.args as { op: { onto: string } }).op.onto),
      ).toEqual(['remote_branch', 'base']),
    );
    await waitFor(() => expect(screen.getByTestId('work-header').dataset.phase).not.toBe('remote_new'));
  });

  it('shows a placeholder for an unknown work item', async () => {
    render(WorkItemHeader, { props: { projectId: 'shop', tabId: 'tab', workItemId: 'nope' } });
    expect(await screen.findByText('Work item not found')).toBeTruthy();
  });
});

describe('return strip', () => {
  const ago = (min: number) => new Date(Date.now() - min * 60_000).toISOString();

  it('shows the next: note, the delta and the last message after 20 min away; x dismisses', async () => {
    const w = { ...item(3), left_at: ago(45) }; // SHOP-155: note, message and delta in the fixtures
    work.upsert(w);
    mountHeader(w);
    const strip = await screen.findByTestId('return-brief');
    await waitFor(() => expect(document.activeElement).toBe(strip)); // x / v reach it, not the pane
    expect(strip.textContent).toContain('next: check the retry path against staging');
    expect(strip.textContent).toMatch(/\+1890\/−122 since\s+you\s+reviewed/);
    expect(strip.textContent).toContain('Added the retry wrapper');
    await fireEvent.keyDown(strip, { key: 'v' });
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_diff')).toBe(true));
    await fireEvent.keyDown(strip, { key: 'x' });
    await waitFor(() => expect(screen.queryByTestId('return-brief')).toBeNull());
  });

  it('not before the threshold, and never while Claude is working', async () => {
    const recent = { ...item(3), left_at: ago(5) };
    work.upsert(recent);
    const { unmount } = mountHeader(recent);
    await screen.findByTestId('work-header');
    expect(screen.queryByTestId('return-brief')).toBeNull();
    unmount();
    // Leaving the item stamps when Louis left.
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_left')).toBe(true));

    const busy = { ...item(0), left_at: ago(120), next_note: 'look at the cache' }; // Claude is working
    work.upsert(busy);
    mountHeader(busy);
    await waitFor(() => expect(screen.getByTestId('work-header').dataset.phase).toBe('working'));
    expect(screen.queryByTestId('return-brief')).toBeNull();
  });
});
