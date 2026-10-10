import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { projects, tickets, toasts } from '$lib/stores';

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
  await projects.load();
});

function mountBoard(mode: 'board' | 'list' = 'board') {
  return render(TicketsPane, {
    props: {
      projectId: 'shop',
      tabId: 'tab-1',
      paneId: 'pane-1',
      content: { kind: 'tickets', scope: { kind: 'project', id: 'shop' }, view_id: null, mode, who: null },
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

  it('moves with the keyboard only: m opens the column menu', async () => {
    const { container } = mountBoard();
    await ready(container);
    await waitFor(() => expect(container.querySelectorAll('[data-column]').length).toBe(4));
    await fireEvent.keyDown(screen.getByTestId('tickets-pane'), { key: 'm' });
    const menu = await screen.findByRole('menu');
    await fireEvent.keyDown(menu, { key: 'ArrowDown' });
    await fireEvent.keyDown(menu, { key: 'Enter' });
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'tracker_move')).toBe(true));
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

  it('shows "No tracker bound" for a project without a tracker', async () => {
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
    expect(await screen.findByText(/^No tracker bound to/)).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Bind a tracker' })).toBeTruthy();
  });
});
