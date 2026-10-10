import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { WorkItem } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { tickets, toasts, work } from '$lib/stores';

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
  await work.load();
});

function mountHeader(item: WorkItem) {
  return render(WorkItemHeader, { props: { projectId: item.project_id, tabId: 'tab', workItemId: item.id } });
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

  it('creates a PR with the edited draft and then offers Open PR', async () => {
    const w = item(0);
    expect(w.pr_url).toBeNull();
    mountHeader(w);
    await fireEvent.click(await screen.findByRole('button', { name: 'Create PR' }));
    await fireEvent.input(screen.getByLabelText('Title'), { target: { value: 'My PR' } });
    await fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Create PR' }));
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_create_pr')).toBe(true));
    expect(mock.calls.filter((c) => c.cmd === 'work_create_pr').at(-1)?.args).toMatchObject({
      id: w.id,
      draft: { title: 'My PR', body: null, draft: null },
    });
    expect(await screen.findByRole('button', { name: 'Open PR' })).toBeTruthy();
  });

  it('lists the dirty files on Finish and only removes with Force', async () => {
    const w = item(1);
    mountHeader(w);
    await fireEvent.click(await screen.findByRole('button', { name: 'Finish' }));
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

  it('rebase: stopped on conflicts → Continue → Rebased → Force push… (confirmed, lease)', async () => {
    const w = item(2); // PR open, Claude idle
    mountHeader(w);
    await fireEvent.click(await screen.findByRole('button', { name: 'Rebase onto main' }));
    expect(await screen.findByText('Rebase stopped (1 conflicted file)')).toBeTruthy();
    for (const name of ['Ask Claude to resolve', 'Open conflicts in nvim', 'Continue', 'Abort rebase']) {
      expect(screen.getByRole('button', { name })).toBeTruthy();
    }
    expect(screen.queryByRole('button', { name: 'Fix with Claude' })).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Continue' }));
    expect(await screen.findByText('Rebased (push rewrites #13)')).toBeTruthy();
    await fireEvent.click(await screen.findByRole('button', { name: 'Force push…' }));
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
    const w = item(1); // billing MR with a suggestion commit on the remote
    const claude = mock.state.sessions.find((x) => w.session_ids.includes(x.id) && x.kind.type === 'claude')!;
    claude.status = 'done'; // it was asking for permission: rebase would be refused
    mountHeader(w);
    expect(await screen.findByText('Remote has new commits (2)')).toBeTruthy();
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
    await waitFor(() => expect(screen.queryByText('Remote has new commits (2)')).toBeNull());
  });

  it('shows a placeholder for an unknown work item', async () => {
    render(WorkItemHeader, { props: { projectId: 'shop', tabId: 'tab', workItemId: 'nope' } });
    expect(await screen.findByText('Work item not found')).toBeTruthy();
  });
});
