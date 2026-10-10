import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { dispatch } from '$lib/actions';
import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { activeTab } from '$lib/layout';
import { layout, projects, sessions, tools, ui } from '$lib/stores';

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
});
