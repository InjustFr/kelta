import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import { dispatch } from '$lib/actions';
import type { GitStatus, Review, WorkItem } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { sessions, tickets, toasts, ui, work } from '$lib/stores';

import '../../shell/actions';
import '../tickets/actions';
import './actions';
import FinishDialog from './FinishDialog.svelte';
import FinishMergedDialog from './FinishMergedDialog.svelte';
import ShipDialog from './ShipDialog.svelte';
import { shipPhase } from './shipPhase';

let mock: MockControls;
const item = (i: number): WorkItem => work.get(mock.state.work[i]!.id)!;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  toasts.clear();
  ui.sheets = [];
  tickets.transitions = {};
  await Promise.all([work.load(), sessions.load()]);
});

const git = (ahead: number): GitStatus => ({ ahead, behind: 0, dirty: false, unpushed: false });

describe('shipPhase (FLOW §2.2 rows 6, 7, 15, 16)', () => {
  const base = (): WorkItem => ({ ...mock.state.work[0]!, pr_url: null, state: { kind: 'active' } });
  const pr = (p: Partial<Review>): Review => ({ ...mock.state.reviews[0]!.review, ...p });

  it('row 6: merged, with the Done move or the choice left', () => {
    const merged = { ...base(), pr_url: 'https://github.com/acme/shop-api/pull/88' };
    expect(shipPhase({ ...merged, state: { kind: 'merged', detail: null } }, null, null)).toMatchObject({
      id: 'merged',
      label: 'Merged',
      detail: '#88, ticket moved to Done',
      primary: 'work.finish',
      section: 'ship',
    });
    const choose = shipPhase(
      { ...merged, state: { kind: 'merged', detail: 'choose Done status' } },
      null,
      null,
    );
    expect(choose?.detail).toBe('choose Done status');
  });

  it('row 7: PR closed unmerged → Finish…', () => {
    const closed = { ...base(), pr_url: 'https://x/pull/13', state: { kind: 'pr_closed' } } as WorkItem;
    expect(shipPhase(closed, null, null)).toMatchObject({
      id: 'pr_closed',
      detail: '#13 closed without merge',
    });
  });

  it('row 15: no PR and commits ahead → Ship; nothing ahead → no ship row', () => {
    expect(shipPhase(base(), null, git(2))).toMatchObject({ id: 'ready_to_ship', detail: '2 commits' });
    expect(shipPhase(base(), null, git(0))).toBeNull();
  });

  it('row 16: approved with passing checks → Open PR (merge there)', () => {
    const open = { ...base(), pr_url: 'https://x/pull/9', state: { kind: 'pr_open' } } as WorkItem;
    expect(shipPhase(open, pr({ decision: 'approved', ci: 'success' }), git(1))).toMatchObject({
      id: 'approved',
      primary: 'open_pr',
    });
    expect(shipPhase(open, pr({ decision: 'approved', ci: 'failure' }), git(1))).toBeNull();
    expect(shipPhase(open, pr({ decision: 'review_required', ci: 'success' }), git(1))).toBeNull();
  });
});

describe('Ship', () => {
  it('is refused while Claude works and disabled with nothing ahead', async () => {
    expect(sessions.get(item(0).session_ids[0]!)?.status).toBe('working');
    const first = render(ShipDialog, { props: { item: item(0), onclose: () => {} } });
    expect(await screen.findByText('Claude is working in this worktree. Ship when it stops.')).toBeTruthy();
    expect(within(screen.getByRole('dialog')).getByRole('button', { name: /^Ship/ })).toHaveProperty(
      'disabled',
      true,
    );
    first.unmount();

    // The failed billing fixture has no commits ahead (and Claude asks for input: blocked first).
    sessions.apply({ type: 'session.removed', id: item(1).session_ids[0]! });
    render(ShipDialog, { props: { item: item(1), onclose: () => {} } });
    expect(await screen.findByText('No commits ahead of main.')).toBeTruthy();
  });

  it('opens with the draft default, toasts Opened PR #n with Open, and ⇧⌘O runs it', async () => {
    const w = item(0);
    sessions.apply({
      type: 'session.updated',
      session: { ...sessions.get(w.session_ids[0]!)!, status: 'done' },
    });
    expect(await dispatch('work.ship', { id: w.id })).toBe(true);
    expect(ui.sheet?.key).toBe('ship');
    render(ShipDialog, { props: { item: w, onclose: () => ui.closeSheet('ship') } });
    await waitFor(() =>
      expect((screen.getByLabelText('Title') as HTMLInputElement).value).toContain('SHOP-142'),
    );
    expect(screen.getByRole('switch', { name: 'Draft' })).toHaveProperty('checked', false);
    expect(screen.getByTestId('ship-dirty').textContent).toContain('will not be in the PR');
    await fireEvent.keyDown(screen.getByLabelText('Title'), { key: 'Enter', metaKey: true });
    await waitFor(() => expect(toasts.list.at(-1)?.toast.text).toBe('Opened PR #100'));
    expect(toasts.lastActionable?.toast.action).toMatchObject({
      label: 'Open',
      command: 'tickets.open_in_browser',
    });
    expect(await dispatch('toast.run_last')).toBe(true);
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'open_external')).toBe(true));
    expect(mock.calls.find((c) => c.cmd === 'open_external')?.args).toEqual({ url: work.get(w.id)!.pr_url });
    expect(toasts.list).toHaveLength(0);
  });

  it('Ask Claude to commit types into the idle Claude session', async () => {
    const w = item(0);
    sessions.apply({
      type: 'session.updated',
      session: { ...sessions.get(w.session_ids[0]!)!, status: 'done' },
    });
    render(ShipDialog, { props: { item: w, onclose: () => {} } });
    await fireEvent.click(await screen.findByRole('button', { name: 'Ask Claude to commit' }));
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'session_write')).toBe(true));
    expect(mock.calls.some((c) => c.cmd === 'work_create_pr')).toBe(false);
  });
});

describe('Finish', () => {
  it('a merged item is prefilled and its Done status is chosen, never guessed', async () => {
    const w = item(3); // billing 4590: merged, "choose Done status"
    render(FinishDialog, { props: { item: w, onclose: () => {} } });
    expect(screen.getByTestId('finish-detail').textContent).toContain('choose Done status');
    expect(screen.getByRole('switch', { name: 'Remove the worktree' })).toHaveProperty('checked', true);
    expect(screen.getByRole('switch', { name: 'Delete the local branch' })).toHaveProperty('checked', true);
    const select = screen.getByLabelText('Done status') as HTMLSelectElement;
    expect(select.value).toBe('');
    await waitFor(() => expect(select.options.length).toBeGreaterThan(1));
    await fireEvent.change(select, { target: { value: select.options[1]!.value } });
    await fireEvent.keyDown(select, { key: 'Enter', ctrlKey: true });
    await waitFor(() => expect(work.get(w.id)?.state.kind).toBe('finished'));
    expect(mock.calls.find((c) => c.cmd === 'work_finish')?.args).toMatchObject({
      opts: { remove_worktree: true, delete_branch: true, transition_to: { name: select.options[1]!.value } },
    });
  });

  it('Finish all merged lists clean items and skips dirty ones and Done choices', async () => {
    render(FinishMergedDialog, { props: { onclose: () => {} } });
    const ready = await screen.findByRole('list', { name: 'To finish' });
    expect(ready.textContent).toContain('SHOP-120');
    const skipped = screen.getByRole('list', { name: 'Skipped' });
    expect(skipped.textContent).toContain('choose Done status');
    expect(skipped.textContent).toContain('wip/cache-warmup');
    expect(skipped.textContent).toContain('uncommitted changes');
    await fireEvent.click(screen.getByRole('button', { name: 'Finish 1' }));
    await waitFor(() =>
      expect(toasts.list.at(-1)?.toast.text).toBe('Finished 1 merged work item, 2 skipped'),
    );
    expect(work.get(mock.state.work[4]!.id)?.state.kind).toBe('finished');
    expect(work.get(mock.state.work[5]!.id)?.state.kind).toBe('merged');
    // Exactly the listed items: the backend never finishes one the dialog did not show.
    expect(mock.calls.find((c) => c.cmd === 'work_finish_merged')?.args).toEqual({
      ids: [mock.state.work[4]!.id],
    });
  });
});
