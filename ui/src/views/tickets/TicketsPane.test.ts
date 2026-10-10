import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { projects, reviews, tickets, toasts, ui, work } from '$lib/stores';

import { selection } from '../work/selection.svelte';
import TicketsPane from './TicketsPane.svelte';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport({ latencyMs: 15 });
  mock = created.controls;
  setTransport(created.transport);
  tickets.lists = {};
  tickets.columns = {};
  tickets.details = {};
  tickets.transitions = {};
  toasts.clear();
  reviews.lists = {};
  ui.sheets = [];
  work.byId = {};
  await projects.load();
});

function mountBoard(mode: 'board' | 'list' = 'board', projectId = 'shop') {
  return render(TicketsPane, {
    props: {
      projectId,
      tabId: 'tab-1',
      paneId: 'pane-1',
      content: { kind: 'tickets', scope: { kind: 'project', id: projectId }, view_id: null, mode, who: null },
      visible: true,
      focused: true,
    },
  });
}

const card = (c: HTMLElement, key: string) => c.querySelector<HTMLElement>(`[data-key="jira-acme:${key}"]`);
const laneOf = (c: HTMLElement, key: string) =>
  card(c, key)?.closest<HTMLElement>('[data-column]')?.dataset.column;

async function ready(c: HTMLElement, key = 'SHOP-151'): Promise<HTMLElement> {
  await waitFor(() => expect(card(c, key)).not.toBeNull());
  const el = card(c, key) as HTMLElement;
  await fireEvent.click(el);
  return el;
}

const shiftRight = (): Promise<boolean> =>
  fireEvent.keyDown(screen.getByTestId('tickets-pane'), { key: 'ArrowRight', shiftKey: true });

describe('TicketsPane board', () => {
  it('builds the columns from tracker_columns and places the cards', async () => {
    const { container } = mountBoard();
    await ready(container);
    await waitFor(() =>
      expect(
        [...container.querySelectorAll<HTMLElement>('[data-column]')].map((l) => l.dataset.column),
      ).toEqual(['todo', 'in_progress', 'in_review', 'done']),
    );
    expect(laneOf(container, 'SHOP-151')).toBe('todo');
    expect(laneOf(container, 'SHOP-142')).toBe('in_progress');
    expect(laneOf(container, 'SHOP-120')).toBe('in_review');
    expect(mock.calls.some((c) => c.cmd === 'tracker_columns')).toBe(true);
  });

  it('moves optimistically and keeps the move once the backend confirms', async () => {
    const { container } = mountBoard();
    await ready(container);
    await waitFor(() => expect(container.querySelectorAll('[data-column]').length).toBe(4));
    await shiftRight();
    await tick();
    expect(laneOf(container, 'SHOP-151')).toBe('in_progress'); // before the backend answered
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'tracker_move')).toBe(true));
    await new Promise((r) => setTimeout(r, 60));
    expect(laneOf(container, 'SHOP-151')).toBe('in_progress');
    expect(mock.calls.filter((c) => c.cmd === 'tracker_move').at(-1)?.args).toMatchObject({
      column_id: 'in_progress',
      ticket: { key: 'SHOP-151' },
      project_id: 'shop',
    });
  });

  it('rolls the card back and shows a toast when the backend fails', async () => {
    const { container } = mountBoard();
    await ready(container);
    await waitFor(() => expect(container.querySelectorAll('[data-column]').length).toBe(4));
    mock.failNext('tracker_move', { code: 'internal', message: 'jira exploded' });
    await shiftRight();
    await tick();
    expect(laneOf(container, 'SHOP-151')).toBe('in_progress');
    await waitFor(() => expect(laneOf(container, 'SHOP-151')).toBe('todo'));
    expect(toasts.list.some((t) => t.toast.level === 'error' && t.toast.text.includes('jira exploded'))).toBe(
      true,
    );
  });

  it('shows the picker when several transitions lead to the column', async () => {
    const { container } = mountBoard();
    await ready(container);
    await waitFor(() => expect(container.querySelectorAll('[data-column]').length).toBe(4));
    mock.failNext('tracker_move', {
      code: 'conflict',
      message: 'ambiguous',
      detail: {
        candidates: [
          { id: 'to-in_progress', name: 'Start progress' },
          { id: 'other', name: 'Resume' },
        ],
      },
    });
    await shiftRight();
    const dialog = await screen.findByRole('dialog', { name: /Move SHOP-151 to In progress/ });
    expect(dialog).toBeTruthy();
    expect(laneOf(container, 'SHOP-151')).toBe('todo'); // rolled back while the user decides
    await fireEvent.click(screen.getByRole('button', { name: 'Start progress' }));
    await waitFor(() => expect(laneOf(container, 'SHOP-151')).toBe('in_progress'));
    expect(mock.calls.filter((c) => c.cmd === 'tracker_transition').at(-1)?.args).toMatchObject({
      transition_id: 'to-in_progress',
    });
  });

  it('asks for the missing fields (NeedsFields), validates and submits them', async () => {
    const { container } = mountBoard();
    await ready(container);
    await waitFor(() => expect(container.querySelectorAll('[data-column]').length).toBe(4));
    mock.failNext('tracker_move', {
      code: 'needs_fields',
      message: 'Done needs a resolution',
      detail: {
        transition_id: 'to-done',
        fields: [{ id: 'resolution', name: 'Resolution', required: true }],
      },
    });
    await shiftRight();
    const input = await screen.findByLabelText('Resolution');
    expect(screen.getByText('Done needs a resolution')).toBeTruthy();

    const submit = async (): Promise<void> => {
      const btn = await screen.findByRole('button', { name: 'Move' });
      await waitFor(() => expect(btn.hasAttribute('disabled')).toBe(false));
      await fireEvent.click(btn);
    };
    await submit();
    expect(await screen.findByText('Resolution is required')).toBeTruthy();
    expect(mock.calls.some((c) => c.cmd === 'tracker_transition')).toBe(false);

    await fireEvent.input(input, { target: { value: 'Fixed' } });
    await submit();
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'tracker_transition')).toBe(true));
    expect(mock.calls.filter((c) => c.cmd === 'tracker_transition').at(-1)?.args).toMatchObject({
      transition_id: 'to-done',
      fields: { resolution: 'Fixed' },
    });
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    await waitFor(() => expect(laneOf(container, 'SHOP-151')).toBe('done'));
  });

  it('toasts "No transition" with an Open in browser action', async () => {
    const { container } = mountBoard();
    await ready(container);
    await waitFor(() => expect(container.querySelectorAll('[data-column]').length).toBe(4));
    mock.failNext('tracker_move', { code: 'not_found', message: 'no transition' });
    await shiftRight();
    await waitFor(() => expect(toasts.list.length).toBe(1));
    const toast = toasts.list[0]?.toast;
    expect(toast?.text).toMatch(/No transition to In progress/);
    expect(toast?.action).toMatchObject({ label: 'Open in browser', command: 'tickets.open_in_browser' });
    expect(laneOf(container, 'SHOP-151')).toBe('todo');
  });

  it('moves with the keyboard only: m opens the transition menu, Enter takes the first', async () => {
    const { container } = mountBoard();
    await ready(container);
    await waitFor(() => expect(container.querySelectorAll('[data-column]').length).toBe(4));
    await fireEvent.keyDown(screen.getByTestId('tickets-pane'), { key: 'm' });
    const menu = await screen.findByRole('menu', { name: 'Move SHOP-151' });
    await screen.findAllByRole('menuitem'); // the menu opens before its moves load
    await fireEvent.keyDown(menu, { key: 'ArrowDown' });
    await fireEvent.keyDown(menu, { key: 'Enter' });
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'tracker_transition')).toBe(true));
    await waitFor(() => expect(laneOf(container, 'SHOP-151')).toBe('in_progress'));
  });
});

describe('TicketsPane states', () => {
  it('filters with / and shows the empty filter state', async () => {
    const { container } = mountBoard('list');
    await waitFor(() => expect(card(container, 'SHOP-151')).not.toBeNull());
    const filter = screen.getByLabelText('Filter tickets') as HTMLInputElement;
    await fireEvent.keyDown(screen.getByTestId('tickets-pane'), { key: '/' });
    expect(document.activeElement).toBe(filter);
    await fireEvent.input(filter, { target: { value: 'flaky' } });
    await waitFor(() => expect(card(container, 'SHOP-151')).toBeNull());
    expect(card(container, 'SHOP-155')).not.toBeNull();
    await fireEvent.input(filter, { target: { value: 'zzzz' } });
    expect(await screen.findByText('No tickets match "zzzz".')).toBeTruthy();
  });

  it('shows the error state with a retry when nothing could be loaded', async () => {
    mock.failAlways('tracker_list', { code: 'network', message: 'offline' });
    mountBoard('list');
    expect(await screen.findByText('Could not load tickets')).toBeTruthy();
    mock.clearFailures();
    await fireEvent.click(screen.getByRole('button', { name: /retry/i }));
    await waitFor(() => expect(screen.queryByText('Could not load tickets')).toBeNull());
  });

  it('clears the work.start selection when the pane unmounts', async () => {
    const { container, unmount } = mountBoard('list');
    await ready(container);
    expect(selection.ticket?.key).toBe('SHOP-151');
    unmount();
    expect(selection.ticket).toBeNull();
  });

  it('shows "No ticket source" with Add source for a project without a tracker', async () => {
    render(TicketsPane, {
      props: {
        projectId: 'home',
        tabId: 't',
        paneId: 'p',
        content: {
          kind: 'tickets',
          scope: { kind: 'project', id: 'home' },
          view_id: null,
          mode: 'list',
          who: null,
        },
        visible: true,
        focused: true,
      },
    });
    expect(await screen.findByText('No ticket source for this project.')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Add source' }));
    expect(ui.sheet).toMatchObject({ key: 'tracker.source_picker', props: { projectId: 'home' } });
  });

  it('says why an empty Unassigned list is empty and offers Anyone', async () => {
    mountBoard('list', 'billing');
    await screen.findByRole('tab', { name: /^Mine/ });
    await fireEvent.keyDown(screen.getByTestId('tickets-pane'), { key: '2' });
    expect(await screen.findByText('Every ticket in Billing has an owner.')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Show anyone' }));
    await waitFor(() =>
      expect(screen.getByRole('tab', { name: /^Anyone/ }).getAttribute('aria-selected')).toBe('true'),
    );
  });
});

const pane = () => screen.getByTestId('tickets-pane');
const press = (key: string, init: KeyboardEventInit = {}) => fireEvent.keyDown(pane(), { key, ...init });
const groupNames = (c: HTMLElement) =>
  [...c.querySelectorAll<HTMLElement>('[data-group]')].map((g) => g.textContent?.replace(/\s+/g, ' ').trim());

describe('TicketsPane workbench', () => {
  it('switches who with 1/2/3, loads with that who and shows the counts', async () => {
    const { container } = mountBoard('list');
    await waitFor(() => expect(card(container, 'SHOP-151')).not.toBeNull());
    await waitFor(() => expect(screen.getByRole('tab', { name: 'Mine 2' })).toBeTruthy());
    await waitFor(() => expect(screen.getByRole('tab', { name: 'Unassigned 1' })).toBeTruthy());
    await press('2');
    await waitFor(() => expect(card(container, 'SHOP-151')).toBeNull());
    expect(card(container, 'SHOP-155')).not.toBeNull();
    expect(screen.getByRole('tab', { name: 'Unassigned 1' }).getAttribute('aria-selected')).toBe('true');
    await press('3');
    await waitFor(() =>
      expect(mock.calls.filter((c) => c.cmd === 'tracker_list').at(-1)?.args).toMatchObject({
        who: 'anyone',
      }),
    );
    await waitFor(() => expect(card(container, 'SHOP-120')).not.toBeNull());
  });

  it('groups by native status, cycles the grouping with g, and keeps Done collapsed', async () => {
    const { container } = mountBoard('list');
    await waitFor(() => expect(card(container, 'SHOP-151')).not.toBeNull());
    expect(groupNames(container)).toEqual(['In Progress 1', 'In Review 1', 'To Do 2']);
    await press('g');
    await waitFor(() =>
      expect(groupNames(container)).toEqual(['Ada Lovelace 2', 'Bob Martin 1', 'Unassigned 1']),
    );
    await press('g'); // source
    await press('g'); // none
    await waitFor(() => expect(groupNames(container)).toEqual([]));
    expect(card(container, 'SHOP-151')).not.toBeNull();
  });

  it('opens a collapsed Done group with Enter', async () => {
    const { container } = mountBoard('list', 'billing');
    const key = (k: string) => container.querySelector(`[data-key="redmine-corp:${k}"]`);
    await waitFor(() => expect(key('4590')).not.toBeNull());
    const done = container.querySelector<HTMLElement>('[data-group="Resolved"]');
    expect(done?.getAttribute('aria-expanded')).toBe('false');
    expect(key('4602')).toBeNull();
    await press('End');
    await press('Enter');
    await waitFor(() => expect(key('4602')).not.toBeNull());
  });

  it('m shows the flow strip and a digit moves at once', async () => {
    const { container } = mountBoard('list');
    await ready(container);
    await press('m');
    const menu = await screen.findByRole('menu', { name: 'Move SHOP-151' });
    await screen.findAllByRole('menuitem');
    const current = menu.querySelector('[aria-current="step"]');
    expect(current?.textContent).toBe('To Do');
    expect([...menu.querySelectorAll('[role="menuitem"] .label')].map((l) => l.textContent)).toEqual([
      'In progress',
      'In review',
      'Done',
    ]);
    await fireEvent.keyDown(menu, { key: '2' });
    await waitFor(() =>
      expect(mock.calls.filter((c) => c.cmd === 'tracker_transition').at(-1)?.args).toMatchObject({
        transition_id: 'to-in_review',
      }),
    );
    expect(screen.queryByRole('menu')).toBeNull();
    await waitFor(() =>
      expect(toasts.list.some((t) => t.toast.text === 'Moved SHOP-151 to In review')).toBe(true),
    );
  });

  it('a digit typed while the moves load is applied once they arrive', async () => {
    const { container } = mountBoard('list');
    await ready(container);
    await press('m');
    const menu = screen.getByRole('menu', { name: 'Move SHOP-151' });
    expect(menu.textContent).toContain('Loading…');
    await fireEvent.keyDown(menu, { key: '2' });
    await waitFor(() =>
      expect(mock.calls.filter((c) => c.cmd === 'tracker_transition').at(-1)?.args).toMatchObject({
        transition_id: 'to-in_review',
      }),
    );
  });

  it('A unassigns the selected ticket', async () => {
    const { container } = mountBoard('list');
    await ready(container);
    await press('A');
    await waitFor(() =>
      expect(mock.calls.filter((c) => c.cmd === 'tracker_assign').at(-1)?.args).toMatchObject({
        ticket: { key: 'SHOP-151' },
        assignee: { kind: 'none' },
      }),
    );
  });

  it('s opens the start sheet, S starts with no sheet', async () => {
    const { container } = mountBoard('list', 'kelta-tools');
    await waitFor(() => expect(container.querySelector('[data-key="github-oss:#15"]')).not.toBeNull());
    await fireEvent.click(container.querySelector('[data-key="github-oss:#15"]') as HTMLElement);
    await press('s');
    await waitFor(() => expect(ui.sheet?.key).toBe('start_work'));
    ui.sheets = [];
    await press('S', { shiftKey: true });
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_start')).toBe(true));
    expect(ui.sheet).toBeNull();
  });

  it('shows the PR chip of a ticket linked by a review', async () => {
    const { container } = mountBoard('list');
    await waitFor(() => {
      const chip = card(container, 'SHOP-120')?.querySelector('[data-pr]');
      expect(chip?.textContent?.trim()).toBe('#305');
      expect(chip?.querySelector('[data-attention="done"]')).not.toBeNull(); // CI success = dot
    });
    expect(card(container, 'SHOP-151')?.querySelector('[data-pr]')).toBeNull();
  });
});
