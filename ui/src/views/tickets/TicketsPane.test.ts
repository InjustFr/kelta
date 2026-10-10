import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { sheetRegistry } from '$app/registry';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import type { PaneContent } from '$lib/gen';
import { layout, projects, reviews, settings, tickets, toasts, ui, work } from '$lib/stores';

import type * as workActions from '../work/actions';
import { runPrimary } from '../work/actions';
import { selection } from '../work/selection.svelte';
import TicketsPane from './TicketsPane.svelte';

vi.mock('../work/actions', async (orig) => ({
  ...(await orig<typeof workActions>()),
  runPrimary: vi.fn(async () => {}),
}));

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
  layout.byProject = {};
  settings.effective = {};
  await projects.load();
});

function mountBoard(
  mode: 'board' | 'list' = 'board',
  projectId = 'shop',
  extra: Partial<Extract<PaneContent, { kind: 'tickets' }>> = {},
) {
  return render(TicketsPane, {
    props: {
      projectId,
      tabId: 'tab-1',
      paneId: 'pane-1',
      content: {
        kind: 'tickets',
        scope: { kind: 'project', id: projectId },
        view_id: null,
        mode,
        who: null,
        ...extra,
      },
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
  // A transitions reload from the previous test's move can land after beforeEach.
  tickets.transitions = {};
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

  it('"Add source…" in the source menu opens the source picker sheet for the project', async () => {
    mountBoard('list');
    await fireEvent.click(await screen.findByRole('button', { name: /All sources/ }));
    await fireEvent.click(await screen.findByRole('menuitem', { name: 'Add source…' }));
    expect(ui.sheet).toMatchObject({ key: 'tracker.source_picker', props: { projectId: 'shop' } });
    const Sheet = (await sheetRegistry['tracker.source_picker']()).default;
    render(Sheet, { props: { ...ui.sheet!.props, onclose: () => ui.closeSheet() } });
    expect(await screen.findByText('Add a ticket source')).toBeTruthy();
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
    await waitFor(() => expect(screen.getByRole('tab', { name: 'Mine 3' })).toBeTruthy());
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

  it('groups by flow by default, cycles the grouping with Shift+g, and keeps Done collapsed', async () => {
    const { container } = mountBoard('list');
    await waitFor(() => expect(card(container, 'SHOP-151')).not.toBeNull());
    expect(groupNames(container)).toEqual(['Doing 1', 'Waiting 2', 'Ready 2']);
    await press('G');
    expect(groupNames(container)).toEqual(['Blocked 1', 'In Progress 1', 'In Review 1', 'To Do 2']);
    await press('G');
    expect(groupNames(container)).toEqual(['High 2', 'Medium 2', 'Low 1']);
    await press('G');
    expect(groupNames(container)).toEqual(['SHOP Sprint 12 4', 'SHOP Sprint 13 1']);
    await press('G');
    await waitFor(() =>
      expect(groupNames(container)).toEqual(['Ada Lovelace 3', 'Bob Martin 1', 'Unassigned 1']),
    );
    await press('G'); // source
    await press('G'); // none
    await waitFor(() => expect(groupNames(container)).toEqual([]));
    expect(card(container, 'SHOP-151')).not.toBeNull();
  });

  it('opens a collapsed Done group with Enter', async () => {
    const { container } = mountBoard('list', 'billing');
    const key = (k: string) => container.querySelector(`[data-key="redmine-corp:${k}"]`);
    await waitFor(() => expect(key('4590')).not.toBeNull());
    const done = container.querySelector<HTMLElement>('[data-group="done"]');
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
      'Blocked',
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
    const { container } = mountBoard('list', 'billing');
    await waitFor(() => expect(container.querySelector('[data-key="redmine-corp:4610"]')).not.toBeNull());
    await fireEvent.click(container.querySelector('[data-key="redmine-corp:4610"]') as HTMLElement);
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
      // CI passed but changes requested: one lamp, the review wins; the label names both
      const lamp = chip?.querySelector('[data-attention="needs_input"]');
      expect(lamp?.getAttribute('aria-label')).toBe('CI passed, changes requested');
    });
    expect(card(container, 'SHOP-151')?.querySelector('[data-pr]')).toBeNull();
  });
});

const keysIn = (c: HTMLElement) =>
  [...c.querySelectorAll<HTMLElement>('[data-group], [data-key]')].map(
    (e) => e.dataset.group ?? e.dataset.key,
  );
const row = (c: HTMLElement, key: string, account = 'jira-acme') =>
  c.querySelector<HTMLElement>(`[data-key="${account}:${key}"]`) as HTMLElement;
const savedContent = () => {
  const root = layout.get('shop')?.tabs[0]?.root;
  return root?.type === 'pane' && root.content.kind === 'tickets' ? root.content : null;
};

/** The pane inside a real layout, so persisted fields can be read back. */
function inLayout(): void {
  layout.byProject = {
    shop: {
      project_id: 'shop',
      active_tab: 'tab-1',
      rev: 0,
      tabs: [
        {
          id: 'tab-1',
          title: 'Tickets',
          work_item_id: null,
          focused_pane: 'pane-1',
          zoomed_pane: null,
          root: {
            type: 'pane',
            id: 'pane-1',
            content: {
              kind: 'tickets',
              scope: { kind: 'project', id: 'shop' },
              view_id: null,
              mode: 'list',
              who: null,
            },
          },
        },
      ],
    },
  } as unknown as typeof layout.byProject;
}

describe('TicketsPane list: group, sort, age, sprint, person', () => {
  it('persists the grouping and the sort with the pane', async () => {
    inLayout();
    const { container } = mountBoard('list');
    await waitFor(() => expect(row(container, 'SHOP-151')).not.toBeNull());
    await press('G');
    await fireEvent.change(screen.getByLabelText('Sort'), { target: { value: 'key' } });
    expect(savedContent()).toMatchObject({ group: 'status', sort: 'key' });
  });

  it("opens with the pane's grouping and sorts within each group", async () => {
    const { container } = mountBoard('list', 'shop', { group: 'priority', sort: 'key' });
    await waitFor(() => expect(row(container, 'SHOP-151')).not.toBeNull());
    expect(keysIn(container)).toEqual([
      '1',
      'jira-acme:SHOP-142',
      'jira-acme:SHOP-160',
      '2',
      'jira-acme:SHOP-120',
      'jira-acme:SHOP-151',
      '3',
      'jira-acme:SHOP-155',
    ]);
  });

  it('shows the status age at 7, 14 and 21 days', async () => {
    const { container } = mountBoard('list');
    await waitFor(() => expect(row(container, 'SHOP-151')).not.toBeNull());
    const age = (k: string, acc?: string) => row(container, k, acc).querySelector<HTMLElement>('[data-age]');
    expect(age('SHOP-142')).toBeNull(); // 3 days
    expect(age('SHOP-151')?.dataset.age).toBe('old'); // 10 days
    expect(age('SHOP-151')?.textContent).toBe('10d');
    expect(age('SHOP-155')?.dataset.age).toBe('danger'); // 25 days
    expect(age('SHOP-155')?.title).toBe('25 days in To Do');
  });

  it('turns the Doing header amber above tickets.wip_limit', async () => {
    const { container } = mountBoard('list');
    await waitFor(() => expect(row(container, 'SHOP-142')).not.toBeNull());
    const doing = () => container.querySelector<HTMLElement>('[data-group="doing"]');
    await waitFor(() => expect(settings.value()).not.toBeNull());
    expect(doing()?.classList.contains('wip')).toBe(false); // 1 in Doing, default limit 3
    const global = settings.effective[''];
    const value = settings.value()!;
    settings.effective = {
      '': { ...global!, data: { ...global!.data!, value: { ...value, tickets: { wip_limit: 0 } } as never } },
    };
    await tick();
    expect(doing()?.classList.contains('wip')).toBe(true);
    expect(doing()?.textContent).toContain('Above your limit of 0');
  });

  it('f s keeps the current sprint; rows carry a sprint chip unless grouped by sprint', async () => {
    const { container } = mountBoard('list');
    await waitFor(() => expect(row(container, 'SHOP-155')).not.toBeNull());
    expect(row(container, 'SHOP-142').querySelector('.sprint')?.textContent).toBe('SHOP Sprint 12');
    await press('f');
    await press('s');
    await waitFor(() => expect(row(container, 'SHOP-155')).toBeNull());
    expect(row(container, 'SHOP-142')).not.toBeNull();
    expect(screen.getByRole('button', { name: /Current sprint/ }).getAttribute('aria-pressed')).toBe('true');
    await fireEvent.click(screen.getByRole('button', { name: /Current sprint/ }));
    await waitFor(() => expect(row(container, 'SHOP-155')).not.toBeNull());
    await press('f');
    await press('j'); // not a chord: j moves on
    expect(row(container, 'SHOP-155')).not.toBeNull();
    for (let i = 0; i < 3; i++) await press('G'); // sprint
    await waitFor(() => expect(container.querySelector('[data-group="SHOP Sprint 12"]')).toBeNull());
    expect(row(container, 'SHOP-142').querySelector('.sprint')).toBeNull();
  });

  it('narrows to a person from the Who control, on top of Anyone, and persists it', async () => {
    inLayout();
    const { container } = mountBoard('list');
    await waitFor(() => expect(row(container, 'SHOP-151')).not.toBeNull());
    await fireEvent.change(screen.getByLabelText('Person'), { target: { value: 'u-bob' } });
    await waitFor(() => expect(row(container, 'SHOP-120')).not.toBeNull());
    expect(row(container, 'SHOP-151')).toBeNull();
    expect(screen.getByRole('tab', { name: /^Anyone/ }).getAttribute('aria-selected')).toBe('true');
    expect(savedContent()).toMatchObject({ person: 'u-bob', who: 'anyone' });
    await press('1'); // a who tab clears the person
    expect(savedContent()).toMatchObject({ person: null, who: 'mine' });
  });
});

describe('TicketsPane list: PRs, status chip, selection, row actions', () => {
  it('p opens the only PR in Kelta, P in the browser, several PRs ask', async () => {
    const { container } = mountBoard('list', 'billing');
    await waitFor(() => expect(row(container, '4567', 'redmine-corp')).not.toBeNull());
    await fireEvent.click(row(container, '4567', 'redmine-corp'));
    await press('P', { shiftKey: true });
    await waitFor(() =>
      expect(mock.calls.filter((c) => c.cmd === 'open_external').at(-1)?.args).toMatchObject({
        url: expect.stringContaining('merge_requests/42'),
      }),
    );
    await press('p');
    await waitFor(() => expect(JSON.stringify(layout.get('billing'))).toContain('"review_detail"'));
  });

  it('several PRs open a picker, from p or the row PR chip', async () => {
    const { container } = mountBoard('list');
    await waitFor(() => expect(row(container, 'SHOP-142')).not.toBeNull());
    const chip = row(container, 'SHOP-142').querySelector<HTMLElement>('[data-pr] [role="button"]')!;
    expect(chip.getAttribute('aria-label')).toBe('Open #309 (p)');
    await fireEvent.click(chip);
    const menu = await screen.findByRole('menu', { name: 'Open pull request' });
    expect(menu.querySelectorAll('[role="menuitem"]').length).toBe(2);
    await fireEvent.click(screen.getByRole('menuitem', { name: /#98/ }));
    await waitFor(() => expect(JSON.stringify(layout.get('shop'))).toContain('"number":98'));
  });

  it('the row status chip opens the status picker', async () => {
    const { container } = mountBoard('list');
    await waitFor(() => expect(row(container, 'SHOP-151')).not.toBeNull());
    await fireEvent.click(
      row(container, 'SHOP-151').querySelector<HTMLElement>('[data-status] [role="button"]')!,
    );
    expect(await screen.findByRole('menu', { name: 'Move SHOP-151' })).toBeTruthy();
  });

  it('x and Shift+j select rows; m moves them all through the moves they share', async () => {
    const { container } = mountBoard('list');
    await ready(container, 'SHOP-151');
    await press('x');
    await press('J', { shiftKey: true }); // extends to SHOP-155
    expect(row(container, 'SHOP-151').classList.contains('picked')).toBe(true);
    expect(row(container, 'SHOP-155').classList.contains('picked')).toBe(true);
    await press('m');
    const menu = await screen.findByRole('menu', { name: 'Move 2 tickets' });
    await screen.findAllByRole('menuitem');
    await fireEvent.keyDown(menu, { key: '1' });
    await waitFor(() =>
      expect(
        mock.calls
          .filter((c) => c.cmd === 'tracker_transition')
          .map((c) => (c.args as { ticket: { key: string } }).ticket.key),
      ).toEqual(['SHOP-151', 'SHOP-155']),
    );
    await waitFor(() => expect(container.querySelector('.picked')).toBeNull());
  });

  it('a multi-move that stops early says how many moved and keeps the selection', async () => {
    const { container } = mountBoard('list');
    await ready(container, 'SHOP-151');
    await press('x');
    await press('J', { shiftKey: true });
    await press('m');
    const menu = await screen.findByRole('menu', { name: 'Move 2 tickets' });
    await screen.findAllByRole('menuitem');
    mock.failNext('tracker_transition', { code: 'internal', message: 'jira exploded' });
    await fireEvent.keyDown(menu, { key: '1' });
    await waitFor(() =>
      expect(toasts.list.map((t) => t.toast.text)).toContain('Moved 0 of 2 tickets to In progress'),
    );
    expect(mock.calls.filter((c) => c.cmd === 'tracker_transition').length).toBe(1);
    expect(container.querySelectorAll('.picked').length).toBe(2);
  });

  it('x toggles, Ctrl+click adds, Esc clears the selection', async () => {
    const { container } = mountBoard('list');
    await ready(container, 'SHOP-151');
    await press('x');
    await fireEvent.click(row(container, 'SHOP-142'), { ctrlKey: true });
    expect(container.querySelectorAll('.picked').length).toBe(2);
    await press('x'); // the cursor is on SHOP-142 now
    expect(container.querySelectorAll('.picked').length).toBe(1);
    await press('Escape');
    expect(container.querySelectorAll('.picked').length).toBe(0);
  });

  it('shows compact row actions; a missing PR disables its action with the reason', async () => {
    const { container } = mountBoard('list');
    await ready(container, 'SHOP-151');
    const acts = row(container, 'SHOP-151').querySelector<HTMLElement>('[data-acts]')!;
    const pr = acts.querySelector('[aria-label="No pull request linked"]');
    expect(pr?.getAttribute('aria-disabled')).toBe('true');
    await fireEvent.click(pr!);
    expect(screen.queryByRole('menu')).toBeNull();
    await fireEvent.click(acts.querySelector('[aria-label="Move (m)"]')!);
    expect(await screen.findByRole('menu', { name: 'Move SHOP-151' })).toBeTruthy();
    await fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' });
    await fireEvent.click(acts.querySelector('[aria-label="Start work (s)"]')!);
    await waitFor(() => expect(ui.sheet?.key).toBe('start_work'));
  });

  it('a read-only tracker refuses assign and comment with the reason', async () => {
    render(TicketsPane, {
      props: {
        projectId: 'shop',
        tabId: 't',
        paneId: 'p',
        content: { kind: 'tickets', scope: { kind: 'all' }, view_id: null, mode: 'list', who: 'anyone' },
        visible: true,
        focused: true,
      },
    });
    const key = await waitFor(() => {
      const el = document.querySelector<HTMLElement>('[data-key="gitlab-corp:#88"]');
      expect(el).not.toBeNull();
      return el!;
    });
    await fireEvent.click(key);
    await press('a');
    await press('c');
    expect(mock.calls.some((c) => c.cmd === 'tracker_assign')).toBe(false);
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(toasts.list.map((t) => t.toast.text)).toEqual([
      "gitlab-corp can't change assignees from Kelta. Open #88 in the browser (o).",
      "gitlab-corp can't take comments from Kelta. Open #88 in the browser (o).",
    ]);
  });
});

describe('TicketsPane split view', () => {
  let width = 1000;
  beforeEach(() => {
    width = 1000;
    Object.defineProperty(HTMLElement.prototype, 'clientWidth', { configurable: true, get: () => width });
    return () => void delete (HTMLElement.prototype as { clientWidth?: number }).clientWidth;
  });
  const detail = () => screen.queryByTestId('ticket-detail');

  it('follows the selection; Space toggles it, Enter focuses it, Esc returns to the list', async () => {
    const { container } = mountBoard('list');
    await ready(container, 'SHOP-151');
    await waitFor(() => expect(detail()?.getAttribute('aria-label')).toBe('Ticket SHOP-151'));
    await press('j');
    await waitFor(() => expect(detail()?.getAttribute('aria-label')).toBe('Ticket SHOP-155'));
    await press(' ');
    expect(detail()).toBeNull();
    await press(' ');
    expect(detail()).not.toBeNull();
    await press('Enter');
    await waitFor(() => expect(document.activeElement).toBe(detail()));
    await fireEvent.keyDown(detail()!, { key: 'j' }); // the detail's keys stay in the detail
    expect(detail()?.getAttribute('aria-label')).toBe('Ticket SHOP-155');
    await fireEvent.keyDown(detail()!, { key: 'Escape' });
    expect(document.activeElement).toBe(pane());
    expect(JSON.stringify(layout.get('shop') ?? null)).not.toContain('"ticket_detail"');
  });

  it('Shift+Enter opens the standalone pane; a narrow pane opens it from Space and Enter', async () => {
    const wide = mountBoard('list');
    await ready(wide.container, 'SHOP-151');
    await press('Enter', { shiftKey: true });
    await waitFor(() => expect(JSON.stringify(layout.get('shop'))).toContain('"ticket_detail"'));
    wide.unmount();
    layout.byProject = {};
    width = 600;
    const { container } = mountBoard('list');
    await ready(container, 'SHOP-151');
    expect(detail()).toBeNull();
    await press(' ');
    await waitFor(() => expect(JSON.stringify(layout.get('shop'))).toContain('"ticket_detail"'));
  });

  it('a row with work shows its phase; Enter runs the next step with the detail closed, g opens it', async () => {
    await work.load();
    const w = work.forTicket({ account: 'jira-acme', key: 'SHOP-142', id: '10142' })!;
    work.upsert({ ...w, review_due: true });
    const { container } = mountBoard('list');
    await ready(container, 'SHOP-142');
    expect(row(container, 'SHOP-142').querySelector('[data-phase]')?.textContent).toBe('To review');
    expect(row(container, 'SHOP-151').querySelector('[data-phase]')).toBeNull();
    await press('Enter'); // detail shown: Enter focuses it
    await waitFor(() => expect(document.activeElement).toBe(detail()));
    await fireEvent.keyDown(detail()!, { key: 'Escape' });
    await press(' ');
    expect(detail()).toBeNull();
    await press('Enter');
    expect(vi.mocked(runPrimary)).toHaveBeenCalledWith(expect.objectContaining({ id: w.id }));
    await press('g');
    await waitFor(() => expect(document.activeElement).toBe(detail()));
  });
});
