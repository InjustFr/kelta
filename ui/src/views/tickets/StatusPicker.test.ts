import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { Ticket } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { tickets, toasts } from '$lib/stores';

import StatusPicker from './StatusPicker.svelte';

let mock: MockControls;
const ticket = (key: string): Ticket => mock.state.tickets.find((i) => i.ticket.ref.key === key)!.ticket;

beforeEach(() => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  tickets.transitions = {};
  tickets.lists = {};
  tickets.details = {};
  toasts.clear();
});

function mount(list: Ticket[]) {
  const onclose = vi.fn();
  render(StatusPicker, { props: { tickets: list, projectId: 'shop', anchor: { x: 10, y: 10 }, onclose } });
  return onclose;
}

const filter = () => screen.getByRole('textbox', { name: 'Filter statuses' });
const labels = async () =>
  (await screen.findAllByRole('menuitem')).map((o) => o.textContent?.replace(/\s+/g, ' ').trim());
const transitionCalls = () => mock.calls.filter((c) => c.cmd === 'tracker_transition');

describe('StatusPicker', () => {
  it('shows the workflow strip, numbers the moves and a digit typed in the filter moves at once', async () => {
    const onclose = mount([ticket('SHOP-151')]);
    expect(await labels()).toEqual(['In progress 1', 'In review 2', 'Done asks for fields 3', 'Blocked 4']);
    expect(
      screen.getByRole('menu', { name: 'Move SHOP-151' }).querySelector('[aria-current="step"]')?.textContent,
    ).toBe('To Do');
    await waitFor(() => expect(document.activeElement).toBe(filter()));
    await fireEvent.keyDown(filter(), { key: '2' });
    await waitFor(() =>
      expect(transitionCalls().at(-1)?.args).toMatchObject({
        ticket: { key: 'SHOP-151' },
        transition_id: 'to-in_review',
      }),
    );
    await waitFor(() => expect(onclose).toHaveBeenCalled());
    expect(toasts.list.at(-1)?.toast.text).toBe('Moved SHOP-151 to In review');
  });

  it('filters fuzzily (letters stay in the filter) and Enter moves to the first match', async () => {
    mount([ticket('SHOP-151')]);
    await labels();
    await fireEvent.input(filter(), { target: { value: 'blk' } });
    expect(await labels()).toEqual(['Blocked 1']);
    await fireEvent.keyDown(filter(), { key: 'Enter' });
    await waitFor(() =>
      expect(transitionCalls().at(-1)?.args).toMatchObject({ transition_id: 'to-blocked' }),
    );
  });

  it('says when nothing matches the filter', async () => {
    mount([ticket('SHOP-151')]);
    await labels();
    await fireEvent.input(filter(), { target: { value: 'qqq' } });
    expect(await screen.findByText('No status matches "qqq".')).toBeTruthy();
  });

  it('moves a selection only to statuses every ticket can reach, by name, skipping those already there', async () => {
    mount([ticket('SHOP-151'), ticket('4567')]); // Jira To Do + Redmine In Progress
    expect(await labels()).toEqual(['In progress 1']);
    expect(screen.getByRole('menu', { name: 'Move 2 tickets' })).toBeTruthy();
    await fireEvent.keyDown(filter(), { key: '1' });
    await waitFor(() => expect(transitionCalls()).toHaveLength(1));
    expect(transitionCalls()[0]?.args).toMatchObject({
      ticket: { key: 'SHOP-151' },
      transition_id: 'to-in_progress',
    });
  });

  it("shows the tracker's message and Open in browser when the transitions cannot load", async () => {
    mock.failNext('tracker_transitions', { code: 'upstream', message: 'Jira is down for maintenance' });
    mount([ticket('SHOP-151')]);
    expect(await screen.findByText('Jira is down for maintenance')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Open in browser' }));
    await waitFor(() =>
      expect(mock.calls.find((c) => c.cmd === 'open_external')?.args).toEqual({
        url: ticket('SHOP-151').url,
      }),
    );
  });

  it('a refused move toasts the tracker message with Open in browser', async () => {
    mount([ticket('4567')]); // Redmine refuses In Progress -> Resolved
    expect(await labels()).toEqual(['New 1', 'Resolved 2']);
    await fireEvent.keyDown(filter(), { key: '2' });
    await waitFor(() => expect(toasts.list.length).toBe(1));
    expect(toasts.list[0]?.toast).toMatchObject({
      level: 'error',
      text: 'Moving 4567 failed: Redmine: status transition not allowed (422)',
      action: {
        label: 'Open in browser',
        command: 'tickets.open_in_browser',
        args: { url: ticket('4567').url },
      },
    });
  });

  it('keeps the fields form open until the move is done, then closes', async () => {
    const onclose = mount([ticket('SHOP-151')]);
    await labels();
    await fireEvent.keyDown(filter(), { key: '3' }); // Jira Done asks for a resolution
    await fireEvent.input(await screen.findByLabelText('Resolution'), { target: { value: 'Fixed' } });
    expect(onclose).not.toHaveBeenCalled();
    const btn = await screen.findByRole('button', { name: 'Move' });
    await waitFor(() => expect(btn.hasAttribute('disabled')).toBe(false));
    await fireEvent.click(btn);
    await waitFor(() => expect(onclose).toHaveBeenCalled());
    expect(transitionCalls().at(-1)?.args).toMatchObject({
      transition_id: 'to-done',
      fields: expect.any(Object),
    });
  });
});
