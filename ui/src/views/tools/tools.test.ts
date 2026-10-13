import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { dispatch } from '$lib/actions';
import type { ToolScope } from '$lib/gen';
import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { activeTab, allPanes, findSession, paneSession } from '$lib/layout';
import { GLOBAL, layout, projects, sessions, tools, ui } from '$lib/stores';

import { noteFocus } from '../../shell/jumplist';
import { activateProject, currentTab, focusedPane, focusPaneById, selectTab } from '../../shell/nav';
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
    // The layout edits stay local: saved, never reloaded over (a reload would drop the zoom).
    const open = async (ctx?: object) => {
      await dispatch('tools.open', { tool_id: id, ...(ctx ? { ctx } : {}) });
      await Promise.all([sessions.load(), layout.flush()]);
    };
    const opens = () => m.calls.filter((c) => c.cmd === 'tool_open');
    return { m, id, open, opens };
  }

  /** The focused pane's session, when it is a tool's. */
  const frontTool = () => {
    const sid = focusedPane() ? paneSession(focusedPane()!) : null;
    return sid && sessions.get(sid)?.kind.type === 'tool' ? sid : null;
  };

  it('opens zoomed in the current tab with its work item (the worktree cwd); the key hides it and brings it back', async () => {
    const { open, opens } = await inTab('W1');
    const tab = currentTab()!;
    const before = allPanes(tab.root).map((p) => p.id);
    await open();
    expect(opens()).toHaveLength(1);
    expect(opens()[0].args).toMatchObject({
      project_id: 'shop',
      ctx: { work_item_id: 'W1' },
      placement: 'split_right',
    });
    const sid = frontTool()!;
    expect(sid).toBeTruthy();
    expect(currentTab()!.id).toBe(tab.id);
    expect(currentTab()!.zoomed_pane).toBe(focusedPane()!.id);

    // Pressed again in front: back to the previous layout, the session keeps running.
    await open();
    expect(allPanes(currentTab()!.root).map((p) => p.id)).toEqual(before);
    expect([currentTab()!.zoomed_pane, frontTool()]).toEqual([null, null]);
    expect(sessions.get(sid)?.lifecycle).toBe('live');

    // Again: the same session comes back zoomed, no second open.
    await open();
    expect(opens()).toHaveLength(1);
    expect(frontTool()).toBe(sid);
    expect(currentTab()!.zoomed_pane).toBe(focusedPane()!.id);

    // Esc Esc (`tools.hide`) hides it too.
    await dispatch('tools.hide');
    expect(frontTool()).toBeNull();
  });

  it('opens a terminal tool in the current tab whatever its placement; another work item gets its own', async () => {
    const { m, open, opens } = await inTab('W1');
    m.state.tools.find((t) => t.kind === 'pty')!.placement = 'new_tab';
    await tools.load('shop');
    const tab = currentTab()!.id;
    await open();
    expect(opens()[0].args).toMatchObject({ placement: 'split_right' });
    expect(currentTab()!.id).toBe(tab);
    await open(); // hides it
    await open({ work_item_id: 'W2' });
    expect(opens()).toHaveLength(2);
  });

  it('with no work item matches only no-work-item instances', async () => {
    const { open, opens } = await inTab(null);
    await open({ work_item_id: 'W1' });
    const other = allPanes(currentTab()!.root).find((p) => p.id !== focusedPane()!.id)!;
    focusPaneById('shop', currentTab()!.id, other.id);
    await open();
    expect(opens()).toHaveLength(2);
    expect(opens()[1].args).toMatchObject({ ctx: { work_item_id: null }, placement: 'split_right' });
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
      await Promise.all([sessions.load(), layout.flush()]);
    };
    await pick();
    expect(opens()).toHaveLength(1);
    expect(opens()[0].args).toMatchObject({ ctx: { work_item_id: 'W1' } });
    await pick();
    expect(opens()).toHaveLength(1);
    expect(frontTool()).toBeTruthy();
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

  it('pressed again in front, a terminal tool that takes a project hides, no picker', async () => {
    const { open, opens } = await needsProject();
    await open();
    expect(frontTool()).toBeTruthy();
    await open();
    expect([ui.pick, frontTool()]).toEqual([null, null]);
    expect(opens()).toHaveLength(1);
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
    await Promise.all([
      layout.ensure('shop'),
      layout.ensure('home'),
      layout.ensure('billing'),
      tools.load('shop'),
    ]);
    const open = async (args: object = {}) => {
      await dispatch('tools.open', { tool_id: id, ...args });
      await Promise.all([sessions.load(), layout.flush()]);
    };
    const opens = () => c.calls.filter((x) => x.cmd === 'tool_open');
    return { c, id, open, opens };
  }

  it('user: a terminal tool opens in the current tab of the project shown, with no work item', async () => {
    const { open, opens } = await scoped('user');
    const tab = currentTab()!.id;
    await open();
    expect(opens()[0].args).toMatchObject({
      project_id: 'shop',
      placement: 'split_right',
      ctx: { work_item_id: null },
    });
    expect([projects.activeId, currentTab()!.id]).toEqual(['shop', tab]);
  });

  it('project: one instance per project, whatever the work item; the key moves it to the current tab', async () => {
    const { open, opens } = await scoped('project');
    await open();
    expect(opens()[0].args).toMatchObject({
      project_id: 'shop',
      placement: 'split_right',
      ctx: { work_item_id: null },
    });
    const sid = paneSession(focusedPane()!)!;
    const other = layout.get('shop')!.tabs.find((t) => t.id !== currentTab()!.id)!;
    selectTab('shop', other.id);
    await open({ ctx: { work_item_id: 'W2' } });
    expect(opens()).toHaveLength(1);
    expect(findSession(layout.get('shop')!, sid)?.tabId).toBe(other.id);
    expect(currentTab()!.zoomed_pane).toBe(focusedPane()!.id);

    await activateProject('billing');
    await open();
    expect(opens()).toHaveLength(2);
    expect(opens()[1].args).toMatchObject({ project_id: 'billing' });
  });

  it('project: from Home or a global view it asks which project', async () => {
    const { open, opens } = await scoped('project');
    ui.view = 'reviews';
    const done = open();
    await vi.waitFor(() => expect(ui.pick).not.toBeNull());
    ui.resolvePick('billing');
    await done;
    expect(opens()[0].args).toMatchObject({ project_id: 'billing', placement: 'split_right' });
  });

  it('a user web tool keeps its own tab in Home; pressed again in front it goes back, here to Now', async () => {
    const { c, opens } = await scoped('work');
    const web = c.state.tools.find((t) => t.id === 'sl-web')!;
    Object.assign(web, { scope: 'user', needs_project: false });
    await tools.load('shop');
    ui.view = 'now';
    noteFocus({ project: GLOBAL, tab: 'now', pane: 'now' }); // what the shell records
    await dispatch('tools.open', { tool_id: 'sl-web' });
    await layout.load('home');
    expect(opens()[0].args).toMatchObject({ project_id: 'home', placement: 'new_tab' });
    expect([ui.view, projects.activeId, focusedPane()?.content.kind]).toEqual([null, 'home', 'web']);
    noteFocus({ project: 'home', tab: currentTab()!.id, pane: focusedPane()!.id });
    await dispatch('tools.open', { tool_id: 'sl-web' });
    expect(opens()).toHaveLength(1);
    expect(ui.view).toBe('now');
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
