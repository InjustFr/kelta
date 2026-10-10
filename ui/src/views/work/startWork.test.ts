import { beforeEach, describe, expect, it } from 'vitest';

import { dispatch } from '$lib/actions';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { projects, settings, toasts, ui } from '$lib/stores';

import './actions';
import { selectTicket } from './selection.svelte';
import { reviewLocally, startWorkOnTicket } from './startWork';

let mock: MockControls;
const ticket = { account: 'jira-acme', key: 'SHOP-151', id: '10151' };
const review = { account: 'github-work', repo: 'acme/web', number: 87 };
const planArgs = () =>
  mock.calls
    .filter((c) => c.cmd === 'work_plan')
    .map((c) => (c.args as { project_id: string | null }).project_id);

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  toasts.clear();
  ui.sheets = [];
  await projects.load();
  await settings.load();
});

describe('start entry points', () => {
  it('never fall back to the active project: the backend picks the default', async () => {
    expect(projects.activeId).toBe('shop');
    await startWorkOnTicket(ticket);
    await startWorkOnTicket(ticket, null);
    await reviewLocally(review);
    await startWorkOnTicket(ticket, 'billing');
    expect(planArgs()).toEqual([null, null, null, 'billing']);
  });

  it('work.start on a selected ticket without a project passes null, not the active project', async () => {
    selectTicket(ticket, null);
    await dispatch('work.start');
    expect(planArgs()).toEqual([null]);
    selectTicket(null, null);
  });

  it('a guessed project shows the sheet even with no preview (S)', async () => {
    await startWorkOnTicket(ticket, null, { preview: false });
    expect(ui.sheets.at(-1)?.key).toBe('start_work');
    expect(mock.calls.some((c) => c.cmd === 'work_start')).toBe(false);
  });

  it('a refused start offers its fix as the toast action', async () => {
    const action = { label: 'Add project', command: 'project.new', args: null };
    mock.failNext('work_plan', {
      code: 'invalid_argument',
      message: 'Add a repository to a project first',
      detail: { action },
    });
    await startWorkOnTicket(ticket);
    const t = toasts.list.at(-1)?.toast;
    expect(t?.text).toBe('Add a repository to a project first');
    expect(t?.action).toEqual(action);
  });
});
