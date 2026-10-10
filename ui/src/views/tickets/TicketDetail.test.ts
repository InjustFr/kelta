import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { StartWorkPlan, TicketItem } from '$lib/gen';
import * as samples from '$lib/gen/fixtures';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { projects, reviews, tickets, toasts, ui, work } from '$lib/stores';
import { ticketKey } from '$lib/stores/tickets.svelte';

import { sheetRegistry } from '$app/registry';

import { acceptanceCriteria, refines, withCriteria } from './refine.svelte';
import TicketDetail from './TicketDetail.svelte';
import { NO_WORKFLOWS, workflows } from './workflow.svelte';

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
  for (const k of Object.keys(workflows)) delete workflows[k];
  await projects.load();
});

function mount(key: string, embedded = true) {
  render(TicketDetail, { props: { item: itemOf(key), projectId: 'shop', embedded } });
  return screen.getByTestId('ticket-detail');
}

const action = (name: string) =>
  within(screen.getByRole('toolbar', { name: 'Ticket actions' })).getByRole('button', { name });

describe('TicketDetail', () => {
  it('w opens the workflow picker; Enter runs the workflow on the ticket and toasts Open and Peek', async () => {
    const ref = itemOf('SHOP-151').ticket.ref;
    const detail = mount('SHOP-151');
    await waitFor(() => expect(workflows[ticketKey(ref)]).toHaveLength(2));
    await fireEvent.keyDown(detail, { key: 'w' });
    await waitFor(() => expect(ui.sheet?.key).toBe('workflow_run'));
    Element.prototype.scrollIntoView ??= () => {}; // not in jsdom
    const Sheet = (await sheetRegistry.workflow_run()).default;
    render(Sheet, { props: { ...ui.sheet!.props, onclose: () => ui.closeSheet('workflow_run') } });
    const input = screen.getByRole('textbox', { name: 'Run workflow on SHOP-151' });
    expect(screen.getByText('triage')).toBeTruthy();
    await fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() =>
      expect(mock.calls.find((c) => c.cmd === 'workflow_run')?.args).toEqual({
        ticket: ref,
        workflow: 'kelta-ticket-pass',
      }),
    );
    expect(ui.sheet).toBeNull();
    await waitFor(() => expect(toasts.list.at(-1)?.toast.text).toBe('kelta-ticket-pass started on SHOP-151'));
    const toast = toasts.list.at(-1)!.toast;
    expect([toast.action?.command, toast.secondary?.command]).toEqual(['session.reveal', 'session.peek']);
  });

  it('turns Run workflow off with the reason when no workflow is found', async () => {
    mock.state.workflows = [];
    const detail = mount('SHOP-151');
    await waitFor(() => expect(workflows[ticketKey(itemOf('SHOP-151').ticket.ref)]).toEqual([]));
    await fireEvent.click(action('More'));
    const entry = within(screen.getByRole('menu', { name: 'More actions' })).getByRole('menuitem', {
      name: /Run workflow…/,
    });
    expect(entry.getAttribute('aria-disabled')).toBe('true');
    expect(entry.getAttribute('title')).toBe(NO_WORKFLOWS);
    await fireEvent.keyDown(screen.getByRole('menu', { name: 'More actions' }), { key: 'Escape' });
    await fireEvent.keyDown(detail, { key: 'w' });
    expect(toasts.list.at(-1)?.toast.text).toBe(NO_WORKFLOWS);
    expect(ui.sheet).toBeNull();
  });

  it('shows the header from the item at once, then the meta grid, PRs, description and comments', async () => {
    mount('SHOP-142');
    expect(screen.getByRole('heading', { level: 1, name: 'Rate-limit login attempts' })).toBeTruthy();
    expect(screen.getByRole('button', { name: /^Status In Progress/ })).toBeTruthy();
    const bar = screen.getByRole('toolbar', { name: 'Ticket actions' });
    expect(
      within(bar)
        .getAllByRole('button')
        .map((b) => b.getAttribute('aria-keyshortcuts')),
    ).toEqual(['s', 'm', 'p', 'a', 'c', 'Control+Shift+.']);
    expect(action('Open PRs')).toBeTruthy(); // two PRs
    // the rest under More (Mod+.), each with its key
    await fireEvent.keyDown(screen.getByTestId('ticket-detail'), {
      key: '>',
      code: 'Period',
      ctrlKey: true,
      shiftKey: true,
    });
    const more = screen.getByRole('menu', { name: 'More actions' });
    expect(
      within(more)
        .getAllByRole('menuitem')
        .map((b) => b.querySelector('.label')?.textContent),
    ).toEqual(['Unassign', 'Refine with Claude', 'Run workflow…', 'Copy branch', 'Open in browser']);
    await fireEvent.keyDown(more, { key: 'Escape' });
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
    ]) {
      expect(action(name!).getAttribute('aria-disabled')).toBe('true');
      expect(action(name!).title).toBe(reason);
    }
    await fireEvent.click(action('More'));
    const branch = screen.getByRole('menuitem', { name: /^Copy branch/ });
    expect(branch.getAttribute('aria-disabled')).toBe('true');
    expect(branch.title).toBe('No branch yet. Start work first.');
    await fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' });
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
        // the parent has no work item: no pick, the backend picks (not the account's project)
        { project_id: null, source: { kind: 'ticket', ticket: itemOf('SHOP-160').ticket.ref } },
      ]),
    );
  });

  it("s on a sub-task picks its parent's work item project (#198)", async () => {
    work.upsert({ ...samples.workItem, project_id: 'billing', ticket: itemOf('SHOP-142').ticket.ref });
    mount('SHOP-142');
    const sub = await screen.findByRole('region', { name: 'Sub-tasks' });
    await fireEvent.keyDown(within(sub).getByRole('button', { name: /SHOP-160/ }), { key: 's' });
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_plan')).toBe(true));
    expect(mock.calls.find((c) => c.cmd === 'work_plan')!.args).toMatchObject({ project_id: 'billing' });
  });

  it('s on a project-less ticket does not pass the pane project as the caller', async () => {
    render(TicketDetail, { props: { item: itemOf('#88'), projectId: 'shop', embedded: true } });
    const root = screen.getByTestId('ticket-detail');
    await fireEvent.keyDown(root, { key: 's' });
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_plan')).toBe(true));
    expect(mock.calls.find((c) => c.cmd === 'work_plan')!.args).toMatchObject({ project_id: null });
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

describe('withCriteria', () => {
  it('escapes braces so the spawn-time template render keeps them literal', () => {
    const ref = { account: 'a', key: 'X-1' } as TicketItem['ticket']['ref'];
    refines[ticketKey(ref)] = {
      running: false,
      text: '## Acceptance criteria\n- uses {{x}} and {id}',
      posted: false,
    };
    const plan = { claude: { prompt: 'Go' } } as StartWorkPlan;
    expect(withCriteria(plan, ref).claude.prompt).toBe(
      'Go\n\nAcceptance criteria (from the refine):\n- uses {{{{x}}}} and {{id}}',
    );
  });
});
