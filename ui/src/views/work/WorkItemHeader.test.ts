import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import { sheetRegistry, type RegisteredSheetKey } from '$app/registry';
import type { WorkItem } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { tickets, toasts, ui, work } from '$lib/stores';

import './actions';
import WorkItemHeader from './WorkItemHeader.svelte';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  tickets.details = {};
  tickets.transitions = {};
  toasts.clear();
  ui.sheets = [];
  work.byId = {};
  await work.load();
});

function mountHeader(item: WorkItem) {
  return render(WorkItemHeader, { props: { projectId: item.project_id, tabId: 'tab', workItemId: item.id } });
}

/** Renders the sheet the last action opened (the shell's SheetHost does this in the app). */
async function mountSheet(key: RegisteredSheetKey) {
  await waitFor(() => expect(ui.sheet?.key).toBe(key));
  const Sheet = (await sheetRegistry[key]()).default;
  return render(Sheet, { props: { ...ui.sheet!.props, onclose: () => ui.closeSheet(key) } });
}

const item = (i: number): WorkItem => mock.state.work[i] as WorkItem;

describe('WorkItemHeader', () => {
  it('shows status, branch and ahead/behind from work_status', async () => {
    const w = item(0);
    mountHeader(w);
    expect(await screen.findByText(w.branch)).toBeTruthy();
    await waitFor(() => expect(screen.getByTestId('ahead').textContent).toMatch(/↑\d+/));
    expect(screen.getByTestId('behind').textContent).toMatch(/↓\d+/);
    expect(mock.calls.some((c) => c.cmd === 'work_status')).toBe(true);
    await waitFor(() => expect(screen.getByRole('button', { name: /In progress/i })).toBeTruthy());
  });

  it('re-reads ahead/behind when the window gains focus', async () => {
    mountHeader(item(0));
    await waitFor(() => expect(screen.getByTestId('ahead')).toBeTruthy());
    const before = mock.calls.filter((c) => c.cmd === 'work_status').length;
    await fireEvent.focus(window);
    await waitFor(() =>
      expect(mock.calls.filter((c) => c.cmd === 'work_status').length).toBeGreaterThan(before),
    );
  });

  it('ships with the edited title and then offers Open PR', async () => {
    const w = item(0);
    expect(w.pr_url).toBeNull();
    mountHeader(w);
    await fireEvent.click(await screen.findByRole('button', { name: 'Ship' }));
    await mountSheet('ship');
    const title = screen.getByLabelText('Title') as HTMLInputElement;
    await waitFor(() => expect(title.value).toBe('SHOP-142: Rate-limit login attempts'));
    await fireEvent.input(title, { target: { value: 'My PR' } });
    // The fixture worktree is dirty: Ship anyway.
    await fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Ship anyway' }));
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_create_pr')).toBe(true));
    expect(mock.calls.filter((c) => c.cmd === 'work_create_pr').at(-1)?.args).toMatchObject({
      id: w.id,
      draft: { title: 'My PR', draft: false },
    });
    expect(await screen.findByRole('button', { name: 'Open PR' })).toBeTruthy();
  });

  it('lists the dirty files on Finish and only removes with Force', async () => {
    const w = item(1);
    mountHeader(w);
    await fireEvent.click(await screen.findByRole('button', { name: 'Finish…' }));
    await mountSheet('finish');
    const dialog = await screen.findByRole('dialog');
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Finish' }));
    const dirty = await screen.findByTestId('finish-dirty');
    expect(dirty.textContent).toContain('src/invoice.rs');
    expect(dirty.textContent).toContain('tests/rounding.rs');
    expect(mock.calls.filter((c) => c.cmd === 'work_finish').at(-1)?.args).toMatchObject({
      opts: { force: false },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Force remove' }));
    await waitFor(() => expect(mock.calls.filter((c) => c.cmd === 'work_finish')).toHaveLength(2));
    expect(mock.calls.filter((c) => c.cmd === 'work_finish').at(-1)?.args).toMatchObject({
      opts: { force: true },
    });
    await waitFor(() => expect(work.get(w.id)?.state.kind).toBe('finished'));
  });

  it('retries the failed step', async () => {
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
