import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { dispatch } from '$lib/actions';
import type { ToolScope } from '$lib/gen';
import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { activeTab, allPanes } from '$lib/layout';
import { GLOBAL, layout, projects, sessions, tools, ui } from '$lib/stores';

import { jumplist, noteFocus } from '../../shell/jumplist';
import NewSessionSheet from '../../shell/NewSessionSheet.svelte';
import {
  activateProject,
  currentTab,
  focusedPane,
  focusPaneById,
  isToolTab,
  railRows,
  selectTab,
  toolSubLines,
} from '../../shell/nav';
import './actions';
import ToolPicker from './ToolPicker.svelte';

function setup() {
  const mock = createMockTransport();
  setTransport(mock.transport);
  return mock.controls;
}

describe('missing tools', () => {
  it('picker shows the install hint when opening fails on a missing binary', async () => {
    const m = setup();
    await tools.load('shop');
    const t = tools.list('shop')[0];
    m.state.tools.find((x) => x.id === t.id)!.installed = false;
    m.failNext('tool_open', { code: 'internal', message: 'spawn failed' });
    const { getByText, findByText, getAllByText } = render(ToolPicker, {
      onclose: vi.fn(),
      projectId: 'shop',
    });
    await fireEvent.click(getByText(t.label, { selector: '.label' }));
    expect(await findByText(/brew install/)).toBeTruthy();
    expect(getAllByText('Check again').length).toBeGreaterThan(0);
  });

  it('tools.open opens first and checks only after a failed open', async () => {
    const m = setup();
    const id = m.state.tools[0].id;
    projects.list = m.state.projects.map((p) => ({ ...p, active: p.id === 'shop' }));
    await dispatch('tools.open', { tool_id: id });
    expect(m.calls.some((c) => c.cmd === 'tool_check')).toBe(false);

    m.state.tools[0].installed = false;
    m.failNext('tool_open', { code: 'internal', message: 'spawn failed' });
    await dispatch('tools.open', { tool_id: id });
    expect(m.calls.some((c) => c.cmd === 'tool_check')).toBe(true);
    expect(ui.sheet).toMatchObject({
      key: 'tool_picker',
      props: { query: id, checks: { [id]: { installed: false } } },
    });
  });
});

describe('tools.open', () => {
  /** Shop active, its active tab tied to `workItemId`; returns the controls and a pty tool id. */
  async function inTab(workItemId: string | null) {
    const m = setup();
    ui.view = null;
    layout.byProject = {};
    tools.byProject = {};
    // No fixture tool instances: every instance comes from these opens.
    m.state.sessions = m.state.sessions.filter((s) => s.kind.type !== 'tool');
    await projects.load();
    await sessions.load();
    const tab = activeTab(m.state.layouts.shop);
    tab!.work_item_id = workItemId;
    await Promise.all([layout.ensure('shop'), tools.load('shop')]);
    const id = m.state.tools.find((t) => t.kind === 'pty')!.id;
    const open = async (ctx?: object) => {
      await dispatch('tools.open', { tool_id: id, ...(ctx ? { ctx } : {}) });
      await Promise.all([sessions.load(), layout.load('shop')]);
    };
    const opens = () => m.calls.filter((c) => c.cmd === 'tool_open');
    return { m, id, open, opens };
  }

  it('sends the tab work item and the tool placement, then refocuses within the same work item', async () => {
    const { m, open, opens } = await inTab('W1');
    m.state.tools.find((t) => t.kind === 'pty')!.placement = 'new_tab';
    await tools.load('shop');
    await open();
    expect(opens()).toHaveLength(1);
    expect(opens()[0].args).toMatchObject({ ctx: { work_item_id: 'W1' }, placement: 'new_tab' });
    await open();
    expect(opens()).toHaveLength(1);
    // Another work item gets its own instance.
    await open({ work_item_id: 'W2' });
    expect(opens()).toHaveLength(2);
  });

  it('with no work item matches only no-work-item instances', async () => {
    const { open, opens } = await inTab(null);
    await open({ work_item_id: 'W1' });
    await open();
    expect(opens()).toHaveLength(2);
    expect(opens()[1].args).toMatchObject({ ctx: { work_item_id: null }, placement: 'split_right' });
    await open();
    expect(opens()).toHaveLength(2);
  });

  it('picker sends the tab work item and refocuses like tools.open', async () => {
    const { m, id, opens } = await inTab('W1');
    const label = m.state.tools.find((t) => t.id === id)!.label;
    const onclose = vi.fn();
    const pick = async () => {
      const { getByText, unmount } = render(ToolPicker, { onclose, projectId: 'shop' });
      await fireEvent.click(getByText(label, { selector: '.label' }));
      await vi.waitFor(() => expect(onclose).toHaveBeenCalled());
      unmount();
      onclose.mockClear();
      await Promise.all([sessions.load(), layout.load('shop')]);
    };
    await pick();
    expect(opens()).toHaveLength(1);
    expect(opens()[0].args).toMatchObject({ ctx: { work_item_id: 'W1' } });
    await pick();
    expect(opens()).toHaveLength(1);
  });

  it('from a global view opens a no-project tool in Home, with no work item', async () => {
    const { open, opens } = await inTab('W1');
    ui.view = 'now';
    await open();
    expect(ui.view).toBeNull();
    expect(projects.activeId).toBe('home');
    expect(opens()[0].args).toMatchObject({ project_id: 'home', ctx: { work_item_id: null } });
  });

  /** The pty tool now takes a project (`{project.root}` in a template). */
  async function needsProject() {
    const t = await inTab('W1');
    t.m.state.tools.find((x) => x.id === t.id)!.needs_project = true;
    await tools.load('shop');
    return t;
  }

  it('from a global view asks which project a needs_project tool opens in', async () => {
    const { m, id, opens } = await needsProject();
    ui.view = 'tickets';
    const done = dispatch('tools.open', { tool_id: id });
    await vi.waitFor(() => expect(ui.pick).not.toBeNull());
    const label = m.state.tools.find((x) => x.id === id)!.label;
    expect([ui.overlay, ui.pick!.placeholder]).toEqual(['switcher', `Open ${label} for project…`]);
    ui.resolvePick('billing');
    await done;
    expect([ui.overlay, ui.view, projects.activeId]).toEqual([null, null, 'billing']);
    expect(opens()).toHaveLength(1);
    expect(opens()[0].args).toMatchObject({ project_id: 'billing', ctx: { work_item_id: null } });
  });

  it('the Open tool sheet asks for a project for a needs_project tool in Home, and from a global view', async () => {
    const { m, id, opens } = await needsProject();
    const label = m.state.tools.find((x) => x.id === id)!.label;
    for (const [view, projectId] of [
      [null, 'home'],
      ['now', 'shop'],
    ] as const) {
      ui.view = view;
      projects.list = projects.list.map((p) => ({ ...p, active: p.id === projectId }));
      await tools.load(projectId);
      const onclose = vi.fn();
      const { getByText, unmount } = render(ToolPicker, { onclose, projectId });
      await fireEvent.click(getByText(label, { selector: '.label' }));
      await vi.waitFor(() => expect(ui.pick).not.toBeNull());
      expect(onclose).toHaveBeenCalled();
      expect(opens()).toHaveLength(0);
      ui.closeOverlay();
      unmount();
    }
  });

  it('Esc on the project picker opens nothing', async () => {
    const { id, opens } = await needsProject();
    ui.view = 'now';
    const done = dispatch('tools.open', { tool_id: id });
    await vi.waitFor(() => expect(ui.pick).not.toBeNull());
    ui.closeOverlay();
    await done;
    expect(opens()).toHaveLength(0);
    expect(ui.view).toBe('now');
  });

  it('inside a project opens a needs_project tool in the focused work item, no picker', async () => {
    const { open, opens } = await needsProject();
    await open();
    expect(ui.pick).toBeNull();
    expect(opens()[0].args).toMatchObject({ project_id: 'shop', ctx: { work_item_id: 'W1' } });
  });

  it('pick_project asks even inside a project', async () => {
    const { id, opens } = await needsProject();
    const done = dispatch('tools.open', { tool_id: id, pick_project: true });
    await vi.waitFor(() => expect(ui.pick).not.toBeNull());
    ui.resolvePick('shop');
    await done;
    expect(opens()[0].args).toMatchObject({ project_id: 'shop', ctx: { work_item_id: null } });
  });

  it('pressed again in front, a tool with no project argument goes back, here to Now', async () => {
    const { id, opens } = await inTab(null);
    ui.view = 'now';
    noteFocus({ project: GLOBAL, tab: 'now', pane: 'now' }); // what the shell records
    await dispatch('tools.open', { tool_id: id });
    await Promise.all([sessions.load(), layout.load('home')]);
    expect([ui.view, projects.activeId]).toEqual([null, 'home']);
    const pane = focusedPane()!;
    expect(toolSubLines().get(pane.id)?.tool).toBe(id);
    noteFocus({ project: 'home', tab: currentTab()!.id, pane: pane.id });
    await dispatch('tools.open', { tool_id: id });
    expect(opens()).toHaveLength(1);
    expect(ui.view).toBe('now');
  });

  it('a focus-only open (the rail click) on the tool in front stays there', async () => {
    const { id } = await inTab(null);
    ui.view = 'now';
    noteFocus({ project: GLOBAL, tab: 'now', pane: 'now' });
    await dispatch('tools.open', { tool_id: id });
    await Promise.all([sessions.load(), layout.load('home')]);
    const pane = focusedPane()!;
    noteFocus({ project: 'home', tab: currentTab()!.id, pane: pane.id });
    await dispatch('tools.open', { tool_id: id, focus_only: true });
    expect(ui.view).toBeNull();
    expect(focusedPane()?.id).toBe(pane.id);
  });

  it('a focus-only open (the rail click) on a project tool in front stays there, no picker', async () => {
    const { open, id, opens } = await needsProject();
    await open();
    await dispatch('tools.open', { tool_id: id, focus_only: true });
    expect(ui.pick).toBeNull();
    expect(opens()).toHaveLength(1);
  });

  it('pressed again in front, a tool that takes a project asks for another project', async () => {
    const { open, id, opens } = await needsProject();
    await open();
    expect(toolSubLines().get(focusedPane()!.id)?.tool).toBe(id);
    const done = dispatch('tools.open', { tool_id: id });
    await vi.waitFor(() => expect(ui.pick).not.toBeNull());
    ui.resolvePick('billing');
    await done;
    expect(opens()).toHaveLength(2);
    expect(opens()[1].args).toMatchObject({ project_id: 'billing', ctx: { work_item_id: null } });
  });
});

describe('tool scope', () => {
  /** Shop active on a W1 tab, no tool instance yet, the pty tool set to `scope`. */
  async function scoped(scope: ToolScope) {
    const m = createMockTransport();
    setTransport(m.transport);
    const c = m.controls;
    ui.view = null;
    layout.byProject = {};
    tools.byProject = {};
    c.state.sessions = c.state.sessions.filter((s) => s.kind.type !== 'tool');
    activeTab(c.state.layouts.shop)!.work_item_id = 'W1';
    const id = c.state.tools.find((t) => t.kind === 'pty')!.id;
    c.state.tools.find((t) => t.id === id)!.scope = scope;
    await projects.load();
    await sessions.load();
    await Promise.all([layout.ensure('shop'), layout.ensure('home'), tools.load('shop')]);
    const open = async (args: object = {}) => {
      await dispatch('tools.open', { tool_id: id, ...args });
      await Promise.all([
        sessions.load(),
        layout.load('shop'),
        layout.load('home'),
        layout.ensure('billing'),
      ]);
    };
    const opens = () => c.calls.filter((x) => x.cmd === 'tool_open');
    return { c, id, open, opens };
  }

  it('user: one Home tab from a project, a work item or a global view; another open only focuses it', async () => {
    const { open, opens } = await scoped('user');
    await open();
    expect(opens()).toHaveLength(1);
    expect(opens()[0].args).toMatchObject({
      project_id: 'home',
      placement: 'new_tab',
      ctx: { work_item_id: null },
    });
    expect(projects.activeId).toBe('home');
    const tab = currentTab()!;
    expect(isToolTab(tab)).toBe(true);

    await activateProject('shop');
    await open();
    ui.view = 'now';
    await open();
    expect(opens()).toHaveLength(1);
    expect([ui.view, projects.activeId, currentTab()!.id]).toEqual([null, 'home', tab.id]);
  });

  it('a Home tool tab makes New session default to the project last shown, not Home', async () => {
    const { open } = await scoped('user');
    jumplist.list = [{ project: 'shop', tab: 'shop-t1', pane: 'shop-p1' }];
    jumplist.at = 0;
    await open();
    render(NewSessionSheet, { onclose: () => {} });
    expect((screen.getByLabelText('Project') as HTMLSelectElement).value).toBe('shop');
  });

  it('project: one instance per project as its own tab, whatever the work item', async () => {
    const { open, opens } = await scoped('project');
    await open();
    expect(opens()[0].args).toMatchObject({
      project_id: 'shop',
      placement: 'new_tab',
      ctx: { work_item_id: null },
    });
    selectTab('shop', 'shop-t1');
    await open({ ctx: { work_item_id: 'W2' } });
    expect(opens()).toHaveLength(1);
    expect(isToolTab(currentTab()!)).toBe(true);

    await activateProject('billing');
    await open();
    expect(opens()).toHaveLength(2);
    expect(opens()[1].args).toMatchObject({ project_id: 'billing' });
  });

  it('a live instance in no pane does not mask a shown one: the second open focuses, no third session', async () => {
    const { c, open, opens } = await scoped('project');
    await open();
    // Hide the first instance: its tab closes, its session keeps running.
    c.state.layouts.shop.tabs = c.state.layouts.shop.tabs.filter((t) => !isToolTab(t));
    await layout.load('shop');
    await open();
    await open();
    expect(opens()).toHaveLength(2);
  });

  it('a tool the config no longer knows leaves its tab a closable row in its band', async () => {
    const { c, open } = await scoped('project');
    await open();
    const tab = currentTab()!;
    expect(isToolTab(tab)).toBe(true);
    c.state.tools = [];
    await tools.load('shop');
    expect(isToolTab(tab)).toBe(false);
  });

  it('project: from Home or a global view it asks which project', async () => {
    const { open, opens } = await scoped('project');
    ui.view = 'reviews';
    const done = open();
    await vi.waitFor(() => expect(ui.pick).not.toBeNull());
    ui.resolvePick('billing');
    await done;
    expect(opens()[0].args).toMatchObject({ project_id: 'billing', placement: 'new_tab' });
  });

  /** Where the rail shows a tab: under Tools (a sub-line or a `user` tool's row) and/or in its band. */
  function places(tabId: string): string[] {
    const lines = toolSubLines();
    return [
      ...([...lines.values()].some((x) => x.tab.id === tabId) ? ['tools'] : []),
      ...(railRows(lines).some((r) => r.tab.id === tabId) ? ['band'] : []),
    ];
  }

  it('a user tool in Home, a dead session, an unknown tool and a web instance each show in one place', async () => {
    const { c, id, open } = await scoped('user');
    await open();
    const userTab = currentTab()!.id;
    expect(places(userTab)).toEqual(['tools']);

    // A project-scoped web tool opens as its own tab: a Tools sub-line, no band row.
    c.state.tools.find((t) => t.id === 'sl-web')!.scope = 'project';
    await tools.load('shop');
    await activateProject('shop');
    await dispatch('tools.open', { tool_id: 'sl-web' });
    await layout.load('shop');
    const webPane = focusedPane()!;
    const webTab = currentTab()!.id;
    expect(webPane.content.kind).toBe('web');
    expect(toolSubLines().get(webPane.id)?.tool).toBe('sl-web');
    expect(places(webTab)).toEqual(['tools']);

    // Dead session: the Home tab falls back to a band row.
    c.state.sessions.find((s) => s.kind.type === 'tool' && s.kind.tool_id === id)!.lifecycle = 'exited';
    await sessions.load();
    expect(places(userTab)).toEqual(['band']);

    // Unknown tool: the web tab falls back to a band row.
    c.state.tools = c.state.tools.filter((t) => t.id !== 'sl-web');
    await tools.load('shop');
    expect(places(webTab)).toEqual(['band']);
  });

  it('a web tool instance gets focus from the tool key, no second open', async () => {
    const { opens } = await scoped('work');
    await dispatch('tools.open', { tool_id: 'sl-web' });
    await layout.load('shop');
    const tab = currentTab()!;
    const web = focusedPane()!;
    const other = allPanes(tab.root).find((p) => p.id !== web.id)!;
    focusPaneById('shop', tab.id, other.id);
    await dispatch('tools.open', { tool_id: 'sl-web' });
    expect(opens()).toHaveLength(1);
    expect(focusedPane()?.id).toBe(web.id);
  });
});
