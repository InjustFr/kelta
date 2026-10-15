import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import { sheetRegistry } from '$app/registry';

import type { ActionButtonDef, PaneContent, PluginInfo, ScreenDef, TemplateCtx } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { layout, plugins, projects, reviews, sessions, settings, tools, toasts, ui, work } from '$lib/stores';
import { idle } from '$lib/stores/loadable';

import '../actions';
import '../../views/tools/actions';
import { takeKeyRequest } from '../../views/settings/actions';
import { flushJump, noteFocus, noteRecent, RECENT_CAP, recentPlaces } from '../jumplist';
import { rank } from './fuzzy';
import Switcher from './Switcher.svelte';
import {
  actionItems,
  buildItems,
  commandItems,
  GROUP_ORDER,
  itemText,
  parseQuery,
  projectItems,
  recentItems,
  sessionItems,
  workItems,
  settingsItems,
  ticketItems,
  toolItems,
  workflowItems,
} from './sources';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  layout.byProject = {};
  await Promise.all([projects.load(), sessions.load(), work.load(), settings.load()]);
  await tools.load('shop');
  await plugins.load();
  ui.view = null;
});

/** The active project's only tab: one pane showing `content`, the tab owned by `workItemId`. */
function focus(content: PaneContent, workItemId: string | null = null): void {
  const id = projects.activeId!;
  const root = { type: 'pane', id: 'pane-1', content };
  layout.byProject = {
    [id]: {
      project_id: id,
      active_tab: 'tab-1',
      rev: 0,
      tabs: [
        {
          id: 'tab-1',
          title: 'T',
          work_item_id: workItemId,
          focused_pane: 'pane-1',
          zoomed_pane: null,
          root,
        },
      ],
    },
  } as unknown as typeof layout.byProject;
}

const say = (id: string): ActionButtonDef => ({
  id,
  title: id,
  do: [{ action: 'toast', text: 'x', level: null }],
});

/** One enabled plugin `qa` with one ticket, review and work action and two screens. */
function withPlugin(enabled = true): void {
  const base = mock.state.plugins[0] as PluginInfo;
  const screen = (id: string, scope: ScreenDef['scope']): ScreenDef => ({
    id,
    title: id.toUpperCase(),
    icon: null,
    entry: 'x.html',
    scope,
    placement: ['tab'],
    keep_alive: false,
    min_width: null,
  });
  const p = {
    ...base,
    id: 'qa',
    name: 'QA',
    enabled,
    contributes: {
      ...base.contributes,
      commands: [],
      screens: [screen('stats', 'project'), screen('all', 'global')],
      ticket_actions: [say('t')],
      review_actions: [say('r')],
      work_actions: [say('w')],
    },
  };
  plugins.plugins = { ...idle(), data: [p], fetchedAt: Date.now() };
}

const qa = () => commandItems().filter((i) => i.detail === 'QA');
const runs = () =>
  mock.calls
    .filter((c) => c.cmd === 'command_run')
    .map((c) => c.args as { command_id: string; ctx: TemplateCtx });

describe('plugin subject actions in the palette', () => {
  it('lists a ticket action only on a ticket detail pane and runs it with that ticket', async () => {
    withPlugin();
    const ticket = { account: 'jira-acme', key: 'SHOP-142', id: '1' };
    focus({
      kind: 'tickets',
      scope: { kind: 'project', id: 'shop' },
      view_id: null,
      mode: 'list',
      who: null,
    } as PaneContent);
    expect(qa().map((i) => i.label)).toEqual(['Open STATS', 'Open ALL']);
    focus({ kind: 'ticket_detail', ticket });
    const t = qa().find((i) => i.label === 't')!;
    await t.run();
    await waitFor(() => expect(runs()).toHaveLength(1));
    expect(runs()[0]).toMatchObject({ command_id: 'qa/t', ctx: { ticket } });
    // Now hides the project: its focused ticket detail is no subject.
    ui.view = 'now';
    expect(qa().map((i) => i.label)).toEqual(['Open STATS', 'Open ALL']);
  });

  it('runs a ticket action with its work item and a review action with its ref before the detail loads', async () => {
    withPlugin();
    const w = work.all.find((x) => x.ticket && x.state.kind !== 'finished')!;
    focus({ kind: 'ticket_detail', ticket: w.ticket! });
    await qa()
      .find((i) => i.label === 't')!
      .run();
    await waitFor(() => expect(runs()).toHaveLength(1));
    expect(runs()[0]).toMatchObject({
      ctx: { ticket: w.ticket, work_item_id: w.id, extra: { project_id: w.project_id } },
    });
    // No detail loaded: the ref alone, core resolves `{pr.*}` (#267).
    const rv = mock.state.reviews[0]!.review;
    focus({ kind: 'review_detail', review: rv.ref });
    await qa()
      .find((i) => i.label === 'r')!
      .run();
    await waitFor(() => expect(runs()).toHaveLength(2));
    expect(runs()[1]!.ctx).toMatchObject({ review: rv.ref, extra: { project_id: projects.activeId } });
  });

  it('lists a work action only with a focused work item and runs it with the item and its project', async () => {
    withPlugin();
    focus({ kind: 'empty' });
    expect(qa().some((i) => i.label === 'w')).toBe(false);
    const w = work.all[0]!;
    focus({ kind: 'empty' }, w.id);
    await qa()
      .find((i) => i.label === 'w')!
      .run();
    await waitFor(() => expect(runs()).toHaveLength(1));
    expect(runs()[0]).toMatchObject({
      command_id: 'qa/w',
      ctx: { work_item_id: w.id, extra: { project_id: w.project_id } },
    });
  });

  it('opens project screens in the active project and global ones in Home', async () => {
    withPlugin();
    const active = projects.activeId; // the run shows the screen's project
    expect(active).toBe('shop');
    for (const i of qa()) await i.run();
    await waitFor(() => expect(runs()).toHaveLength(2));
    expect(runs().map((r) => [r.command_id, r.ctx.extra.project_id])).toEqual([
      ['qa/open:stats', active],
      ['qa/open:all', 'home'],
    ]);
  });

  it('a disabled plugin contributes nothing', () => {
    withPlugin(false);
    focus({ kind: 'empty' }, work.all[0]!.id);
    expect(qa()).toEqual([]);
  });
});

describe('palette sources', () => {
  it('lists the sessions of every project with their status, needing-input first', () => {
    const items = sessionItems();
    expect(items).toHaveLength(12);
    // Sessions of a work item are told apart by its key.
    expect(items.map((i) => i.label)).toContain('SHOP-155 claude');
    expect(items[0]!.detail).toContain('Needs input');
    expect(items[0]!.detail).toContain('Billing');
    expect(items.every((i) => i.group === 'Sessions')).toBe(true);
  });

  it('lists unfinished work items with their lamp and phase', () => {
    const items = workItems();
    expect(items.map((i) => i.label)).toContain('SHOP-155 feat/SHOP-155-flaky-cart-test');
    const toReview = items.find((i) => i.label.startsWith('SHOP-155'))!;
    expect([toReview.group, toReview.lamp, toReview.detail]).toEqual([
      'Work items',
      'done',
      'To review\u2002\u2002feat/SHOP-155-flaky-cart-test',
    ]);
  });

  it('lists every configured project, open or not', () => {
    expect(projectItems().map((i) => i.label)).toEqual(['Home', 'Shop', 'Billing', 'Kelta tools']);
  });

  it('lists the registered actions with the effective chord of the platform', () => {
    const items = actionItems();
    const zoom = items.find((i) => i.id === 'action:pane.zoom')!;
    expect(zoom.label).toBe('Zoom pane');
    expect(zoom.kbd).toBeTruthy();
    // Actions owned by other lanes are not offered until they are registered; goto.N is omitted.
    expect(items.find((i) => i.id.startsWith('action:project.goto'))).toBeUndefined();
    expect(items.find((i) => i.id === 'action:tickets.open')).toBeUndefined();
    expect(items.map((i) => i.label)).toEqual(
      expect.arrayContaining(['Work: Next step', 'Work: Move to step…']),
    );
  });

  it('lists tools of the active project and settings sections', () => {
    expect(toolItems().map((i) => i.label)).toContain('Open lazygit');
    expect(settingsItems().map((i) => i.label)).toContain('Settings: Keys');
  });

  it('offers "for project…" only for tools that take a project, and it always asks', async () => {
    const labels = toolItems().map((i) => i.label);
    expect(labels).toContain('Open Sapling ISL for project…');
    expect(labels.filter((l) => l.endsWith('for project…'))).toEqual(['Open Sapling ISL for project…']);
    expect(labels).not.toContain('Open lazydocker for project…');
    await projects.activate('shop');
    toolItems()
      .find((i) => i.id === 'tool-for:sl-web')!
      .run();
    await waitFor(() => expect(ui.pick?.placeholder).toBe('Open Sapling ISL for project…'));
    ui.closeOverlay();
  });

  it('the switcher as a project picker leaves Home out and hands the pick over', async () => {
    Element.prototype.scrollIntoView ??= () => {}; // not in jsdom
    const picked: string[] = [];
    const active = projects.activeId;
    render(Switcher, {
      onclose: () => {},
      placeholder: 'Open x for project…',
      onpick: (id: string) => picked.push(id),
    });
    expect(screen.getByPlaceholderText('Open x for project…')).toBeTruthy();
    const options = screen.getAllByRole('option').map((o) => o.querySelector('.label')!.textContent);
    expect(options).toEqual(['Shop', 'Billing', 'Kelta tools']);
    await fireEvent.click(screen.getAllByRole('option')[1]!);
    expect(picked).toEqual(['billing']);
    expect(projects.activeId).toBe(active);
  });

  it('maps ticket search hits to palette items with their project', () => {
    const items = ticketItems([
      {
        ticket: {
          ref: { account: 'jira-acme', key: 'SHOP-1', id: '1' },
          title: 'Rate limit',
          url: 'https://x',
          status: { id: 's', name: 'In progress', category: 'in_progress' },
          kind: null,
          assignee: null,
          labels: [],
          priority: null,
          updated_at: '2026-10-01T00:00:00Z',
          project_hint: null,
          priority_rank: null,
          status_since: null,
          sprint: null,
          estimate: null,
          due: null,
          blocked: false,
        },
        project_ids: ['shop'],
        work_item_id: null,
        view_ids: [],
        source_ids: [],
        prs: [],
        caps: {
          board_columns: false,
          assign: true,
          comment: true,
          transitions_need_fetch: false,
          projects_v2: false,
          block: false,
        },
        waiting: [],
      },
    ]);
    expect(items[0]).toMatchObject({ group: 'Tickets', label: 'SHOP-1 Rate limit' });
    expect(items[0]!.detail).toBe('In progress\u2002\u2002Shop');
  });

  it('offers verbs on the best ticket hits only, each acting on the exact ticket', async () => {
    const hits = mock.state.tickets.slice(0, 5);
    const items = ticketItems(hits);
    const key = hits[0]!.ticket.ref.key;
    expect(items.map((i) => i.label).slice(0, 7)).toEqual([
      `${key} ${hits[0]!.ticket.title}`,
      `Start work on ${key}`,
      `Start another PR on ${key}`, // it has work (#456)
      `Run workflow on ${key}…`,
      `Move ${key} to…`,
      `Assign ${key} to me`,
      `Open ${key} in browser`,
    ]);
    // 3 hits with 5 verbs (one more for each of the two with work) + 2 plain hits
    expect(items).toHaveLength(5 + 3 * 5 + 2);
    expect(new Set(items.map((i) => i.id)).size).toBe(items.length);

    await items.find((i) => i.label === `Assign ${key} to me`)!.run();
    expect(mock.calls.find((c) => c.cmd === 'tracker_assign')?.args).toMatchObject({
      ticket: hits[0]!.ticket.ref,
      assignee: { kind: 'me' },
    });
    await import('../../views/tickets/actions'); // registered late: an earlier test needs tickets.open absent
    await items.find((i) => i.label === `Open ${key} in browser`)!.run();
    expect(mock.calls.find((c) => c.cmd === 'open_external')?.args).toMatchObject({
      url: hits[0]!.ticket.url,
    });
  });

  it('"Move KEY to…" opens the status picker for that ticket; filter and Enter move it', async () => {
    ui.sheets = [];
    const hit = mock.state.tickets[0]!;
    await ticketItems([hit])
      .find((i) => i.label === `Move ${hit.ticket.ref.key} to…`)!
      .run();
    expect(ui.sheet).toMatchObject({
      key: 'tickets.move',
      props: { ticket: hit.ticket, project_id: hit.project_ids[0] },
    });
    const Sheet = (await sheetRegistry['tickets.move']()).default;
    render(Sheet, { props: { ...ui.sheet!.props, onclose: () => ui.closeSheet('tickets.move') } });
    await screen.findByRole('menu', { name: `Move ${hit.ticket.ref.key}` });
    await screen.findAllByRole('menuitem');
    const filter = screen.getByRole('textbox', { name: 'Filter statuses' });
    await fireEvent.input(filter, { target: { value: 'review' } });
    await fireEvent.keyDown(filter, { key: 'Enter' });
    await waitFor(() =>
      expect(mock.calls.find((c) => c.cmd === 'tracker_transition')?.args).toMatchObject({
        ticket: hit.ticket.ref,
        transition_id: 'to-in_review',
      }),
    );
    await waitFor(() => expect(ui.sheet).toBeNull());
  });

  it('"Run workflow on KEY…" opens the workflow picker for that ticket', async () => {
    ui.sheets = [];
    const hit = mock.state.tickets[1]!;
    await ticketItems([hit])
      .find((i) => i.label === `Run workflow on ${hit.ticket.ref.key}…`)!
      .run();
    await waitFor(() =>
      expect(ui.sheet).toMatchObject({
        key: 'workflow_run',
        props: { ticket: hit.ticket.ref, workflows: mock.state.workflows },
      }),
    );
  });

  it('a global config workflow runs from the palette with no ticket; a second run starts another', async () => {
    const item = workflowItems().find((i) => i.label === 'Run Daily report')!;
    expect(workflowItems().map((i) => i.label)).toEqual(['Run Daily report']);
    await item.run();
    await item.run();
    const runs = mock.calls.filter((c) => c.cmd === 'workflow_run_on').map((c) => c.args);
    expect(runs).toEqual([
      { project_id: projects.activeId, workflow: 'daily', review: null },
      { project_id: projects.activeId, workflow: 'daily', review: null },
    ]);
    expect(buildItems().some((i) => i.id === 'workflow:daily')).toBe(true);
  });

  it('offers no assign verb on a tracker that cannot assign', () => {
    const hit = mock.state.tickets.find((t) => !t.caps.assign)!;
    const labels = ticketItems([hit]).map((i) => i.label);
    expect(labels).toContain(`Move ${hit.ticket.ref.key} to…`);
    expect(labels).not.toContain(`Assign ${hit.ticket.ref.key} to me`);
  });

  it('ranks across groups and keeps the display group order', () => {
    const all = buildItems();
    expect(new Set(all.map((i) => i.group)).size).toBeGreaterThan(3);
    for (const g of GROUP_ORDER) expect(GROUP_ORDER.includes(g)).toBe(true);
    const hits = rank(all, 'shop claude', itemText);
    expect(hits[0]!.group).toBe('Sessions');
  });
});

describe('recent places, pull requests, settings keys and prefixes (#420)', () => {
  const ticketPane = (key: string): PaneContent =>
    ({ kind: 'ticket_detail', ticket: { account: 'jira-acme', key, id: key } }) as PaneContent;

  it('lists the last places visited, most recent first, reopening a ticket no list holds', async () => {
    const session = sessions.all[0]!;
    noteRecent({ id: `session:${session.id}`, label: 'x' });
    noteRecent({ id: 'setting:keys', label: 'keys' });
    focus(ticketPane('SHOP-7'));
    noteFocus({ project: projects.activeId!, tab: 'tab-1', pane: 'pane-1' });
    flushJump();
    noteRecent({ id: 'work:gone', label: 'finished' });

    expect(
      recentPlaces()
        .map((r) => r.id)
        .slice(0, 4),
    ).toEqual(['work:gone', 'ticket:jira-acme:SHOP-7', 'setting:keys', `session:${session.id}`]);
    const items = recentItems(buildItems());
    // a work item no longer listed drops out; the others keep their item's label and run
    expect(items.slice(0, 3).map((i) => [i.group, i.id, i.label])).toEqual([
      ['Recent', 'recent:ticket:jira-acme:SHOP-7', 'T'],
      ['Recent', 'recent:setting:keys', 'Settings: Keys'],
      [
        'Recent',
        `recent:session:${session.id}`,
        sessionItems().find((i) => i.id === `session:${session.id}`)!.label,
      ],
    ]);

    focus({ kind: 'empty' });
    await items[0]!.run();
    const tabs = layout.get(projects.activeId!)!.tabs;
    expect(tabs.at(-1)!.root).toMatchObject({ type: 'pane', content: ticketPane('SHOP-7') });
  });

  it('keeps the last 8 places', () => {
    for (let i = 0; i < RECENT_CAP + 3; i += 1) noteRecent({ id: `setting:s${i}`, label: '' });
    expect(recentPlaces()).toHaveLength(RECENT_CAP);
    expect(recentPlaces()[0]!.id).toBe(`setting:s${RECENT_CAP + 2}`);
  });

  it('finds a pull request by its number', async () => {
    await reviews.load({ kind: 'all' }, 'team');
    const hit = rank(buildItems(), '311', itemText)[0]!;
    expect(hit).toMatchObject({ group: 'Pull requests', id: 'review:github-acme:acme/shop-api#311' });
  });

  it('finds a settings key by its label and opens Settings at that key', async () => {
    await settings.loadSchema();
    const hit = rank(buildItems(), 'wip limit', itemText)[0]!;
    expect(hit).toMatchObject({ group: 'Settings', id: 'setting-key:tickets.wip_limit', label: 'Wip limit' });
    expect(hit.detail).toContain('Soft WIP limit');
    expect(hit.value).toBe('3');
    expect(hit.checked).toBeUndefined();
    await import('../../views/settings/actions');
    await hit.run();
    expect(ui.settings).toEqual({ section: 'worktree' });
    expect(takeKeyRequest()).toBe('tickets.wip_limit');
    ui.settings = null;
  });

  it('matches a settings key by a keyword of the copy table', async () => {
    await settings.loadSchema();
    const item = settingsItems().find((i) => i.id === 'setting-key:app.updates')!;
    expect(item.label).toBe('Check for updates automatically');
    expect(itemText(item)).toContain('upgrade');
  });

  it('prefixes narrow the query to their group', () => {
    expect(parseQuery('#311')).toEqual({ groups: ['Tickets'], text: '311' });
    expect(parseQuery('! 311')).toEqual({ groups: ['Pull requests'], text: '311' });
    expect(parseQuery('>split')).toEqual({ groups: ['Actions', 'Commands', 'Tools'], text: 'split' });
    expect(parseQuery(', wip limit ')).toEqual({ groups: ['Settings'], text: 'wip limit' });
    expect(parseQuery(' wip limit')).toEqual({ groups: null, text: 'wip limit' });
  });
});

describe('settings in place (#478)', () => {
  const keyItem = (path: string) => settingsItems().find((i) => i.id === `setting-key:${path}`)!;
  const setCalls = (cmd: string) => mock.calls.filter((c) => c.cmd === cmd);

  it('shows on/off settings as a switch and others with their value', async () => {
    await settings.loadSchema();
    const onOff = keyItem('terminal.copy_on_select');
    expect(onOff.checked).toBe(false);
    expect(onOff.value).toBeUndefined();
    expect(onOff.open).toBeDefined();
    expect(keyItem('tickets.wip_limit').value).toBe('3');
  });

  it('↵ flips an on/off setting in All projects, with Undo', async () => {
    await settings.loadSchema();
    await keyItem('terminal.copy_on_select').run();
    expect(setCalls('settings_set').at(-1)!.args).toMatchObject({
      layer: 'global',
      path: 'terminal.copy_on_select',
      value: true,
    });
    expect((settings.value() as any).terminal.copy_on_select).toBe(true);
    expect(ui.settings).toBeNull();
    const last = toasts.list.at(-1)!;
    expect(last.toast.text).toBe('Copy on select turned on');
    expect(last.toast.action).toMatchObject({
      command: 'settings.restore',
      args: { path: 'terminal.copy_on_select', value: null },
    });
    await toasts.run(last.id);
    expect(setCalls('settings_reset').at(-1)!.args).toMatchObject({ layer: 'global', path: 'terminal.copy_on_select' });
    expect((settings.value() as any).terminal.copy_on_select).toBe(false);
  });

  it('⌘↵ opens an on/off setting on its row in Settings', async () => {
    await settings.loadSchema();
    keyItem('terminal.copy_on_select').open!();
    expect(ui.settings).toEqual({ section: 'terminal' });
    expect(takeKeyRequest()).toBe('terminal.copy_on_select');
    ui.settings = null;
  });
});
