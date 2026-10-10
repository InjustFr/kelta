import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { TicketItem } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { projects, reviews, tickets, toasts, ui, work } from '$lib/stores';

import { acceptanceCriteria, refines } from './refine.svelte';
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
  for (const k of Object.keys(refines)) delete refines[k];
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
    ).toEqual(['s', 'm', 'p', 'a', 'Shift+A', 'c', 'r', 'y', 'o']);
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

  it('drops a half-written comment when the item changes under it (split view)', async () => {
    const { rerender } = render(TicketDetail, {
      props: { item: itemOf('SHOP-142'), projectId: 'shop', embedded: true },
    });
    const box = () => screen.getByLabelText(/Add a comment/) as HTMLTextAreaElement;
    await fireEvent.input(box(), { target: { value: 'draft for SHOP-142' } });
    await rerender({ item: itemOf('SHOP-120'), projectId: 'shop', embedded: true });
    await waitFor(() => expect(box().value).toBe(''));
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

  it('u assigns to anyone the tracker offers, ! changes the priority', async () => {
    const root = mount('SHOP-142');
    await fireEvent.keyDown(root, { key: 'u' });
    const people = await screen.findByRole('menu', { name: 'Assign SHOP-142' });
    await fireEvent.input(within(people).getByLabelText('Find a person'), { target: { value: 'grace' } });
    await waitFor(() =>
      expect(mock.calls.filter((c) => c.cmd === 'tracker_assignable_users').at(-1)?.args).toMatchObject({
        query: 'grace',
      }),
    );
    await waitFor(() => expect(within(people).getAllByRole('menuitem')).toHaveLength(1));
    await fireEvent.click(within(people).getByRole('menuitem', { name: /Grace Hopper/ }));
    await waitFor(() =>
      expect(mock.calls.filter((c) => c.cmd === 'tracker_assign').at(-1)?.args).toMatchObject({
        assignee: { kind: 'user', id: 'u-grace' },
      }),
    );
    expect(toasts.list.at(-1)?.toast.text).toBe('SHOP-142 assigned to Grace Hopper');

    await fireEvent.keyDown(root, { key: '!' });
    const prios = await screen.findByRole('menu', { name: 'Priority of SHOP-142' });
    await fireEvent.click(await within(prios).findByRole('menuitem', { name: 'Low' }));
    await waitFor(() =>
      expect(mock.calls.filter((c) => c.cmd === 'tracker_set_priority').at(-1)?.args).toMatchObject({
        priority: 'Low',
      }),
    );
  });

  it('the priority picker shows why a tracker cannot change it', async () => {
    mount('#88'); // gitlab-corp: read-only tracker
    await fireEvent.click(screen.getByRole('button', { name: 'None' }));
    const menu = await screen.findByRole('menu', { name: 'Priority of #88' });
    expect(await within(menu).findByText('gitlab-corp cannot set priorities')).toBeTruthy();
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

  it('lists sub-tasks with their status; s on one starts work on it, not on the parent', async () => {
    mount('SHOP-142');
    const sub = await screen.findByRole('region', { name: 'Sub-tasks' });
    const row = within(sub).getByRole('button', { name: /SHOP-160/ });
    expect(row.textContent).toContain(itemOf('SHOP-160').ticket.status.name);
    await fireEvent.keyDown(row, { key: 's' });
    await waitFor(() =>
      expect(mock.calls.filter((c) => c.cmd === 'work_plan').map((c) => c.args)).toEqual([
        { project_id: 'shop', source: { kind: 'ticket', ticket: itemOf('SHOP-160').ticket.ref } },
      ]),
    );
  });

  it('c focuses the comment box', async () => {
    const root = mount('SHOP-151');
    await fireEvent.keyDown(root, { key: 'c' });
    expect(document.activeElement).toBe(screen.getByLabelText(/Add a comment/));
  });

  it('r refines with Claude: preview, post as comment, criteria in the start prompt, discard', async () => {
    const root = mount('SHOP-151');
    const ref = itemOf('SHOP-151').ticket.ref;
    await fireEvent.keyDown(root, { key: 'r' });
    const section = await screen.findByTestId('ticket-refine');
    await waitFor(() =>
      expect(section.querySelector('pre')?.textContent).toContain('## Acceptance criteria'),
    );
    expect(mock.calls.find((c) => c.cmd === 'tracker_refine')?.args).toEqual({
      ticket: ref,
      project_id: 'shop',
    });

    await fireEvent.click(within(section).getByRole('button', { name: 'Post as comment' }));
    await waitFor(() =>
      expect(mock.calls.find((c) => c.cmd === 'tracker_comment')?.args).toMatchObject({
        markdown: expect.stringContaining('## Open questions'),
      }),
    );
    await within(section).findByRole('button', { name: 'Posted' });

    const prompt = async () => {
      await fireEvent.keyDown(root, { key: 's' });
      await waitFor(() => expect(ui.sheets.length).toBeGreaterThan(0));
      const sheet = ui.sheets.pop()!;
      return (sheet.props as { plan: { claude: { prompt: string } } }).plan.claude.prompt;
    };
    expect(await prompt()).toContain('Acceptance criteria (from the refine):\n- [ ] ');

    await fireEvent.click(within(section).getByRole('button', { name: 'Discard' }));
    await waitFor(() => expect(screen.queryByTestId('ticket-refine')).toBeNull());
    expect(await prompt()).not.toContain('from the refine');
  });
});

describe('acceptanceCriteria', () => {
  it('takes the section body up to the next heading', () => {
    const md = '# T\n\n## Acceptance criteria\n\n- [ ] a\n- [ ] b\n\n## Open questions\nNone';
    expect(acceptanceCriteria(md)).toBe('- [ ] a\n- [ ] b');
    expect(acceptanceCriteria('### acceptance criteria\n- x')).toBe('- x');
    expect(acceptanceCriteria('## Acceptance criteria\n\n## Open questions')).toBeNull();
    expect(acceptanceCriteria('no section')).toBeNull();
  });
});
