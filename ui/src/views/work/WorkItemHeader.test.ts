import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { WorkItem } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { findContent } from '$lib/layout';
import { layout, projects, sessions, tickets, toasts, work } from '$lib/stores';

import { NOT_YET } from './actions';
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
  Object.assign(workUi, { menu: null, ship: null, finish: null });
  // ProjectRail reads git status at startup; the header itself never asks.
  await Promise.all([work.load(), sessions.load(), projects.load(), work.refreshStatus()]);
});

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
    expect(screen.getByText('dirty')).toBeTruthy();
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

  it('To review: the primary opens the diff zoomed in the work tab, x marks reviewed', async () => {
    const w = item(3); // SHOP-155, review_due
    mountHeader(w);
    await waitFor(() => expect(screen.getByTestId('work-header').dataset.phase).toBe('to_review'));
    await fireEvent.click(screen.getByRole('button', { name: 'Review diff' }));
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
    await fireEvent.keyDown(menu, { key: 'x' });
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
    expect(labels.length).toBe(15); // primary + d p x f r c a n s g l t o ⇧F
    const rebase = entries.find((e) => label(e).startsWith('Rebase onto'))!;
    expect(rebase.getAttribute('aria-disabled')).toBe('true');
    expect(rebase.title).toBe(NOT_YET);
    const mark = entries.find((e) => label(e).startsWith('Mark reviewed'))!;
    expect(mark.title).toBe('Nothing to review');
    await fireEvent.keyDown(menu, { key: 'r' }); // disabled: nothing runs, the menu stays
    expect(screen.getByRole('menu', { name: 'Work' })).toBeTruthy();
  });

  it('review checkouts are read-only: p r f l are gated', async () => {
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
    expect(reasons).toEqual(['Ship', 'Fix', 'Rebase', 'Link']);
  });

  it('p ships through the Create PR dialog', async () => {
    const w = { ...item(0) };
    mock.state.sessions.find((s) => s.id === w.session_ids[0])!.status = 'done';
    await sessions.load();
    mountHeader(w);
    const menu = await openMenu();
    await fireEvent.keyDown(menu, { key: 'p' });
    const dialog = await screen.findByRole('dialog');
    await fireEvent.input(within(dialog).getByLabelText('Title'), { target: { value: 'My PR' } });
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Create PR' }));
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_create_pr')).toBe(true));
    expect(mock.calls.filter((c) => c.cmd === 'work_create_pr').at(-1)?.args).toMatchObject({
      id: w.id,
      draft: { title: 'My PR', body: null, draft: null },
    });
    // PR open with unpushed commits: the next action is Push (another lane's).
    await waitFor(() => expect(screen.getByTestId('work-header').dataset.phase).toBe('unpushed'));
  });

  it('Shift+F finishes: lists the dirty files and only removes with Force', async () => {
    const w = item(1);
    mountHeader(w);
    const menu = await openMenu();
    await fireEvent.keyDown(menu, { key: 'F' });
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

  it('shows a placeholder for an unknown work item', async () => {
    render(WorkItemHeader, { props: { projectId: 'shop', tabId: 'tab', workItemId: 'nope' } });
    expect(await screen.findByText('Work item not found')).toBeTruthy();
  });
});
