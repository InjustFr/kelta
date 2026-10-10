import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { TicketItem } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { projects, reviews, tickets, toasts, ui, work } from '$lib/stores';

import TicketDetail from './TicketDetail.svelte';

let mock: MockControls;
const itemOf = (key: string): TicketItem => mock.state.tickets.find((i) => i.ticket.ref.key === key)!;

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

function mount(key: string, embedded = true) {
  render(TicketDetail, { props: { item: itemOf(key), projectId: 'shop', embedded } });
  return screen.getByTestId('ticket-detail');
}

const action = (name: string) =>
  within(screen.getByRole('toolbar', { name: 'Ticket actions' })).getByRole('button', { name });

describe('TicketDetail', () => {
  it('shows the header from the item at once, then the meta grid, PRs, description and comments', async () => {
    mount('SHOP-142');
    expect(screen.getByRole('heading', { level: 1, name: 'Rate-limit login attempts' })).toBeTruthy();
    expect(screen.getByRole('button', { name: /^Status In Progress/ })).toBeTruthy();
    const bar = screen.getByRole('toolbar', { name: 'Ticket actions' });
    expect(
      within(bar)
        .getAllByRole('button')
        .map((b) => b.getAttribute('aria-keyshortcuts')),
    ).toEqual(['s', 'm', 'p', 'a', 'A', 'c', 'y', 'o']);
    expect(action('Open PRs')).toBeTruthy(); // two PRs
    const meta = screen.getByTestId('ticket-detail').querySelector('dl')!;
    expect(meta.textContent).toContain('SHOP Sprint 12');
    expect(meta.textContent?.replace(/\s+/g, ' ')).toContain('Estimate 5');
    const prs = screen.getByTestId('ticket-pr');
    expect(prs.querySelectorAll('.pr')).toHaveLength(2);
    expect(prs.textContent).toContain('#309');
    expect(prs.textContent).toContain('#98');
    expect(await screen.findByText(/Mock body rendered/)).toBeTruthy();
    expect(screen.getByText('Can we keep the existing behaviour behind a flag?')).toBeTruthy();
    expect(screen.getByLabelText(/Add a comment/)).toBeTruthy();
  });

  it('marks a past due date as overdue', () => {
    mount('SHOP-120'); // due 2026-10-09, in review
    expect(screen.getByText('overdue')).toBeTruthy();
  });

  it('turns off what the tracker cannot do, with the reason, and a key says why', async () => {
    mount('#88'); // gitlab-corp: no assign, no comment, no PR, no branch
    for (const [name, reason] of [
      ['Assign to me', 'This tracker does not let Kelta change the assignee.'],
      ['Comment', 'This tracker does not let Kelta add comments.'],
      ['Open PR', 'No pull request is linked to this ticket yet.'],
      ['Copy branch', 'No branch yet. Start work first.'],
    ]) {
      expect(action(name!).getAttribute('aria-disabled')).toBe('true');
      expect(action(name!).title).toBe(reason);
    }
    expect(action('Move').getAttribute('aria-disabled')).toBeNull();
    expect(screen.queryByLabelText(/Add a comment/)).toBeNull();
    await fireEvent.click(action('Assign to me'));
    await fireEvent.keyDown(screen.getByTestId('ticket-detail'), { key: 'a' });
    expect(mock.calls.some((c) => c.cmd === 'tracker_assign')).toBe(false);
    expect(toasts.list.at(-1)?.toast.text).toBe('This tracker does not let Kelta change the assignee.');
  });

  it('p with several PRs opens the PR picker; P opens the chosen one in the browser', async () => {
    const root = mount('SHOP-142');
    await fireEvent.keyDown(root, { key: 'P' });
    const menu = await screen.findByRole('menu', { name: 'Pull requests of SHOP-142' });
    expect(within(menu).getAllByRole('menuitem')).toHaveLength(2);
    await fireEvent.click(within(menu).getAllByRole('menuitem')[1]!);
    await waitFor(() =>
      expect(mock.calls.find((c) => c.cmd === 'open_external')?.args).toEqual({
        url: itemOf('SHOP-142').prs[1]!.url,
      }),
    );
  });

  it('y copies the branch of the work item', async () => {
    await work.load();
    const root = mount('SHOP-142');
    const branch = work.forTicket(itemOf('SHOP-142').ticket.ref)!.branch;
    await fireEvent.keyDown(root, { key: 'y' });
    await waitFor(() =>
      expect(mock.calls.find((c) => c.cmd === 'clipboard_write')?.args).toEqual({
        kind: 'clipboard',
        text: branch,
      }),
    );
    expect(toasts.list.at(-1)?.toast.text).toBe(`Copied ${branch}`);
  });

  it('the status chip opens the status picker, and the filter plus Enter moves', async () => {
    mount('SHOP-151');
    await fireEvent.click(screen.getByRole('button', { name: /^Status To Do/ }));
    await screen.findAllByRole('menuitem');
    const filter = screen.getByRole('textbox', { name: 'Filter statuses' });
    await fireEvent.input(filter, { target: { value: 'review' } });
    await fireEvent.keyDown(filter, { key: 'Enter' });
    await waitFor(() =>
      expect(mock.calls.filter((c) => c.cmd === 'tracker_transition').at(-1)?.args).toMatchObject({
        transition_id: 'to-in_review',
      }),
    );
  });

  it('c focuses the comment box', async () => {
    const root = mount('SHOP-151');
    await fireEvent.keyDown(root, { key: 'c' });
    expect(document.activeElement).toBe(screen.getByLabelText(/Add a comment/));
  });
});
