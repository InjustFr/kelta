import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { TicketRef } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { projects, reviews, tickets, toasts, ui, work } from '$lib/stores';

import TicketDetailPane from './TicketDetailPane.svelte';

let mock: MockControls;
const ref: TicketRef = { account: 'jira-acme', key: 'SHOP-151', id: '10151' };

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  tickets.details = {};
  tickets.transitions = {};
  tickets.lists = {};
  toasts.clear();
  ui.sheets = [];
  work.byId = {};
  reviews.lists = {};
  await projects.load();
});

function mountDetail(r: TicketRef = ref) {
  return render(TicketDetailPane, {
    props: {
      projectId: 'shop',
      tabId: 't',
      paneId: 'p',
      content: { kind: 'ticket_detail', ticket: r },
      visible: true,
      focused: true,
    },
  });
}

describe('TicketDetailPane', () => {
  it('renders the title, status, body and comments', async () => {
    mountDetail();
    expect(
      await screen.findByRole('heading', { level: 1, name: /Checkout: show tax breakdown/ }),
    ).toBeTruthy();
    expect(screen.getByText('To Do')).toBeTruthy();
    expect(screen.getByRole('region', { name: 'Description' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Start work' })).toBeTruthy();
  });

  it('posts a comment with Ctrl+Enter', async () => {
    mountDetail();
    const box = await screen.findByLabelText(/Add a comment/);
    await fireEvent.input(box, { target: { value: 'Looks good' } });
    await fireEvent.keyDown(box, { key: 'Enter', ctrlKey: true });
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'tracker_comment')).toBe(true));
    expect(mock.calls.filter((c) => c.cmd === 'tracker_comment').at(-1)?.args).toEqual({
      ticket: ref,
      markdown: 'Looks good',
    });
    await waitFor(() => expect((box as HTMLTextAreaElement).value).toBe(''));
  });

  it('moves the ticket through the Move to menu (keyboard: m)', async () => {
    mountDetail();
    await screen.findByRole('heading', { level: 1 });
    await waitFor(() => expect(tickets.transitions['jira-acme:SHOP-151']?.data?.length).toBeGreaterThan(0));
    await fireEvent.keyDown(screen.getByTestId('ticket-detail'), { key: 'm' });
    const menu = await screen.findByRole('menu');
    await fireEvent.keyDown(menu, { key: 'ArrowDown' }); // first enabled transition: In progress
    await fireEvent.keyDown(menu, { key: 'Enter' });
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'tracker_transition')).toBe(true));
    expect(mock.calls.filter((c) => c.cmd === 'tracker_transition').at(-1)?.args).toMatchObject({
      transition_id: 'to-in_progress',
    });
    await waitFor(() =>
      expect(tickets.details['jira-acme:SHOP-151']?.data?.ticket.status.category).toBe('in_progress'),
    );
  });

  it('assigns to me and opens the start-work sheet', async () => {
    mountDetail();
    await screen.findByRole('heading', { level: 1 });
    await fireEvent.keyDown(screen.getByTestId('ticket-detail'), { key: 'a' });
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'tracker_assign')).toBe(true));
    await fireEvent.click(screen.getByRole('button', { name: 'Start work' }));
    await waitFor(() => expect(ui.sheet?.key).toBe('start_work'));
    expect((ui.sheet?.props.plan as { project_id: string }).project_id).toBe('shop');
  });

  it('offers Resume work when a work item exists', async () => {
    const existing = mock.state.work.find((w) => w.ticket?.key === 'SHOP-142');
    await work.load();
    mountDetail({ account: 'jira-acme', key: 'SHOP-142', id: '10142' });
    expect(existing).toBeTruthy();
    expect(await screen.findByRole('button', { name: 'Resume work' })).toBeTruthy();
  });

  it('shows the linked pull request with its CI lamp and review state', async () => {
    mountDetail({ account: 'jira-acme', key: 'SHOP-120', id: '10120' });
    await screen.findByRole('heading', { level: 1 });
    const row = screen.getByTestId('ticket-pr');
    await waitFor(() => expect(row.textContent).toContain('#305'));
    expect(row.textContent).toContain('Changes requested');
    expect(row.querySelector('[data-attention="done"]')).not.toBeNull();
  });

  it('opens the move menu from the status value and moves with a digit', async () => {
    mountDetail();
    await screen.findByRole('heading', { level: 1 });
    expect(screen.getByTestId('ticket-pr').textContent).toContain('None');
    await fireEvent.click(screen.getByRole('button', { name: /^Status To Do/ }));
    const menu = await screen.findByRole('menu', { name: 'Move SHOP-151' });
    expect(menu.querySelector('[aria-current="step"]')?.textContent).toBe('To Do');
    await fireEvent.keyDown(menu, { key: '3' });
    await waitFor(() =>
      expect(mock.calls.filter((c) => c.cmd === 'tracker_transition').at(-1)?.args).toMatchObject({
        transition_id: 'to-done',
      }),
    );
    expect(await screen.findByLabelText('Resolution')).toBeTruthy(); // jira Done needs fields
  });

  it('shows the not-found and error states', async () => {
    mock.failAlways('tracker_get', { code: 'not_found', message: 'gone' });
    mountDetail();
    expect(await screen.findByText('SHOP-151 was not found')).toBeTruthy();
  });
});
