import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { activeTab } from '$lib/layout';
import { layout, projects, sessions, toasts, ui, work } from '$lib/stores';

import { confirms } from './confirm.svelte';
import NewSessionSheet from './NewSessionSheet.svelte';
import {
  activateProject,
  closeFocusedPane,
  currentTab,
  cycleProject,
  cycleTab,
  focusedSessionId,
  gotoRailRow,
  openContent,
  openRuns,
  openView,
  railProjects,
  railRows,
  requestCloseTab,
  revealSession,
  selectRow,
  splitFocused,
  tabAttention,
  toggleZoomFocused,
  viewKey,
  workRows,
} from './nav';

const SHOP_CLAUDE = '0199a6b2-0000-7000-8000-000000000001';
const BILLING_CLAUDE = '0199a6b2-0000-7000-8000-000000000005';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  layout.byProject = {};
  ui.view = null;
  toasts.clear();
  await Promise.all([projects.load(), sessions.load(), work.load()]);
  await layout.ensure('shop');
});

const calls = (cmd: string) => mock.calls.filter((c) => c.cmd === cmd);

describe('projects', () => {
  it('rail order: open projects in list order, Home last', () => {
    expect(railProjects().map((p) => p.id)).toEqual(['shop', 'billing', 'kelta-tools', 'home']);
  });

  it('activates a project without killing or detaching anything', async () => {
    await activateProject('billing');
    expect(projects.activeId).toBe('billing');
    expect(layout.get('billing')).not.toBeNull();
    expect(calls('session_kill')).toEqual([]);
    expect(calls('session_detach')).toEqual([]);
    expect(calls('project_activate')).toHaveLength(1);
  });

  it('records the project switch measure', async () => {
    await activateProject('billing');
    await Promise.resolve();
    expect(performance.getEntriesByName('kelta.project_switch').length).toBeGreaterThan(0);
  });

  it('rail rows: goto N counts work rows across bands, tab next/prev walk every row and wrap around', async () => {
    await Promise.all(['billing', 'kelta-tools', 'home'].map((id) => layout.ensure(id)));
    expect(railRows().map((r) => r.tab.id)).toEqual([
      'shop-t1',
      'shop-t2',
      'shop-t3',
      'billing-t1',
      'billing-t2',
      'kt-t1',
      'kt-t2',
      'home-t1',
      'home-t2',
    ]);
    expect(workRows().map((r) => r.tab.id)).toEqual(['shop-t1', 'billing-t1', 'kt-t1']);
    const at = () => [projects.activeId, currentTab()!.id];
    gotoRailRow(2);
    expect(at()).toEqual(['billing', 'billing-t1']);
    gotoRailRow(4); // no such work row: unchanged
    expect(at()).toEqual(['billing', 'billing-t1']);
    cycleTab(1);
    expect(at()).toEqual(['billing', 'billing-t2']);
    cycleTab(1);
    expect(at()).toEqual(['kelta-tools', 'kt-t1']);
    cycleTab(-1);
    expect(at()).toEqual(['billing', 'billing-t2']);
    selectRow('home', 'home-t2');
    cycleTab(1);
    expect(at()).toEqual(['shop', 'shop-t1']);
    cycleTab(-1);
    expect(at()).toEqual(['home', 'home-t2']);
    // From a global view the next row is the first one.
    openView('now');
    cycleTab(1);
    expect([ui.view, ...at()]).toEqual([null, 'shop', 'shop-t1']);
  });

  it('work rows (#430): a band lists its work by start time, and a phase change does not move a row', async () => {
    const started = '0199a6b2-0000-7000-8000-00000000a004'; // SHOP-155, started before SHOP-142
    layout.update('shop', (l) => ({
      ...l,
      tabs: [...l.tabs, { ...l.tabs[2]!, id: 'shop-t4', title: 'SHOP-155 Coupons', work_item_id: started }],
    }));
    const order = () => workRows().map((r) => r.tab.id);
    expect(order()).toEqual(['shop-t4', 'shop-t1']);
    expect(railRows().map((r) => r.tab.id)).toEqual(['shop-t4', 'shop-t1', 'shop-t2', 'shop-t3']);
    work.apply({ type: 'work.updated', work: { ...work.get(started)!, state: { kind: 'pr_open' } } });
    expect(order()).toEqual(['shop-t4', 'shop-t1']);
    gotoRailRow(1);
    expect(currentTab()!.id).toBe('shop-t4');
  });

  it('project cycling follows the rail order and wraps around', async () => {
    await activateProject('billing');
    cycleProject(1);
    expect(projects.activeId).toBe('kelta-tools');
    cycleProject(1);
    expect(projects.activeId).toBe('home');
    cycleProject(1);
    expect(projects.activeId).toBe('shop');
    cycleProject(-1);
    expect(projects.activeId).toBe('home');
  });

  it('leaves a global view when a project is activated, even the active one', async () => {
    ui.view = 'now';
    await activateProject('shop');
    expect(ui.view).toBeNull();
  });

  it('a global view keeps the active project; next/prev from it go to the first/last project', () => {
    openView('tickets');
    expect(ui.view).toBe('tickets');
    expect(projects.activeId).toBe('shop');
    cycleProject(-1);
    expect([ui.view, projects.activeId]).toEqual([null, 'home']);
    openView('reviews');
    cycleProject(1);
    expect([ui.view, projects.activeId]).toEqual([null, 'shop']);
  });
});

describe('viewKey', () => {
  it('is the global view, else project/tab, else null before the layouts load', async () => {
    await activateProject('shop');
    expect(viewKey()).toBe(`shop/${currentTab()!.id}`);
    openView('now');
    expect(viewKey()).toBe('now');
    layout.byProject = {};
    ui.view = null;
    expect(viewKey()).toBeNull();
  });
});

describe('sessions', () => {
  it('reveals a session of another project: project, tab and pane', async () => {
    expect(await revealSession(BILLING_CLAUDE)).toBe(true);
    expect(projects.activeId).toBe('billing');
    expect(focusedSessionId()).toBe(BILLING_CLAUDE);
  });

  it('reveals a session in another tab of the same project', async () => {
    const shell = '0199a6b2-0000-7000-8000-000000000003';
    await revealSession(shell);
    expect(currentTab()!.id).toBe('shop-t2');
    expect(focusedSessionId()).toBe(shell);
  });

  it('raises the window of a detached tab instead of activating the project', async () => {
    const shell = '0199a6b2-0000-7000-8000-000000000003';
    layout.update('shop', (l) => ({
      ...l,
      tabs: l.tabs.map((t) => (t.id === 'shop-t2' ? { ...t, detached: true } : t)),
    }));
    await revealSession(shell);
    expect(calls('window_detach')).toHaveLength(1);
    expect(currentTab()!.id).not.toBe('shop-t2');
  });

  it('opens a background session in a new tab', async () => {
    const closing = closeFocusedPane(); // shop-p1 (Claude) leaves the layout but keeps running
    await Promise.resolve();
    confirms.answer('background');
    await closing;
    expect(layout.get('shop')!.tabs[0]!.root.type).toBe('pane');
    const tabs = layout.get('shop')!.tabs.length;
    await revealSession(SHOP_CLAUDE);
    expect(layout.get('shop')!.tabs).toHaveLength(tabs + 1);
    expect(focusedSessionId()).toBe(SHOP_CLAUDE);
    expect(calls('session_kill')).toEqual([]);
  });

  it('a work tab shows done from review_due, not from the session seen flag', () => {
    const tab = layout.get('shop')!.tabs[0]!;
    const item = work.get(tab.work_item_id!)!;
    sessions.upsert({ ...sessions.get(SHOP_CLAUDE)!, status: 'done', attention: 'done', seen: false });
    expect(tabAttention(tab)).toBe('none');
    work.upsert({ ...item, review_due: true });
    expect(tabAttention(tab)).toBe('done');
  });

  it('computes tab attention from the sessions shown in the tab', async () => {
    const tab = layout.get('shop')!.tabs[0]!;
    // A work tab: the item's phase (Claude working) outranks its sessions' activity.
    expect(tabAttention(tab)).toBe('working');
    sessions.upsert({ ...sessions.get(SHOP_CLAUDE)!, attention: 'needs_input' });
    expect(tabAttention(tab)).toBe('needs_input');
  });
});

describe('panes and tabs', () => {
  it('splits the focused pane with a shell in the same directory', async () => {
    const before = activeTab(layout.get('shop')!)!;
    await splitFocused('column');
    const spawn = calls('session_spawn')[0]!.args as { req: { cwd: string; kind: { type: string } } };
    expect(spawn.req.kind).toEqual({ type: 'shell' });
    expect(spawn.req.cwd).toBe(sessions.get(SHOP_CLAUDE)!.cwd);
    const after = activeTab(layout.get('shop')!)!;
    expect(after.id).toBe(before.id);
    expect(JSON.stringify(after.root)).not.toBe(JSON.stringify(before.root));
    expect(after.focused_pane).not.toBe(before.focused_pane);
  });

  it('a split from a global view shows the active project with the new pane', async () => {
    openView('tickets');
    await splitFocused('row');
    expect(ui.view).toBeNull();
    expect(calls('session_spawn')).toHaveLength(1);
  });

  it('a new session started from a global view shows its project, even the active one', async () => {
    openView('tickets');
    render(NewSessionSheet, { onclose: () => {} });
    await fireEvent.click(await screen.findByTestId('new-session-start'));
    await waitFor(() => expect(ui.view).toBeNull());
    expect(calls('session_spawn_template')).toHaveLength(1);
  });

  it('toggles zoom on the focused pane', () => {
    toggleZoomFocused();
    expect(currentTab()!.zoomed_pane).toBe(currentTab()!.focused_pane);
    toggleZoomFocused();
    expect(currentTab()!.zoomed_pane).toBeNull();
  });

  it('opening runs un-zooms the session tab, on first open and on re-open', async () => {
    await revealSession(SHOP_CLAUDE);
    toggleZoomFocused();
    expect(currentTab()!.zoomed_pane).not.toBeNull();
    await openRuns(SHOP_CLAUDE);
    expect(currentTab()!.zoomed_pane).toBeNull();
    toggleZoomFocused();
    await openRuns(SHOP_CLAUDE);
    expect(currentTab()!.zoomed_pane).toBeNull();
  });

  it('closing the last pane of a tab closes the tab, never a process', async () => {
    const tabs = layout.get('shop')!.tabs.length;
    for (let i = 0; i < 2; i++) {
      const closing = closeFocusedPane();
      await Promise.resolve();
      confirms.answer('background');
      await closing;
    }
    expect(layout.get('shop')!.tabs).toHaveLength(tabs - 1);
    expect(calls('session_kill')).toEqual([]);
  });

  it('closing a pane with a live session asks: kill, background or cancel', async () => {
    const pane = currentTab()!.focused_pane;
    const cancel = closeFocusedPane();
    await Promise.resolve();
    expect(confirms.current?.actions.map((a) => a.id)).toEqual(['background', 'kill']);
    confirms.answer(null);
    await cancel;
    expect(currentTab()!.focused_pane).toBe(pane);

    const kill = closeFocusedPane();
    await Promise.resolve();
    confirms.answer('kill');
    await kill;
    expect(calls('session_kill').map((c) => (c.args as { id: string }).id)).toEqual([SHOP_CLAUDE]);
  });

  it('closing a pane whose session exited does not ask', async () => {
    sessions.upsert({ ...sessions.get(SHOP_CLAUDE)!, lifecycle: 'exited' });
    await closeFocusedPane();
    expect(confirms.current).toBeNull();
    expect(layout.get('shop')!.tabs[0]!.root.type).toBe('pane');
  });

  it('cycles tabs with wrap-around', () => {
    expect(currentTab()!.id).toBe('shop-t1');
    cycleTab(-1);
    expect(currentTab()!.id).toBe('shop-t3');
    cycleTab(1);
    expect(currentTab()!.id).toBe('shop-t1');
  });

  it('openContent opens a pane in the requested project and activates it', async () => {
    await openContent('billing', { content: { kind: 'diagnostics' }, placement: 'new_tab' });
    expect(projects.activeId).toBe('billing');
    expect(currentTab()!.title).toBe('Diagnostics');
  });

  it('closing a tab with live sessions asks: keep, stop or cancel', async () => {
    const closing = requestCloseTab('shop', 'shop-t2');
    await Promise.resolve();
    expect(confirms.current?.title).toContain('Shell');
    confirms.answer(null);
    await closing;
    expect(layout.get('shop')!.tabs.map((t) => t.id)).toContain('shop-t2');

    const keep = requestCloseTab('shop', 'shop-t2');
    await Promise.resolve();
    confirms.answer('keep');
    await keep;
    expect(layout.get('shop')!.tabs.map((t) => t.id)).not.toContain('shop-t2');
    expect(calls('session_kill')).toEqual([]);

    const stop = requestCloseTab('shop', 'shop-t1');
    await Promise.resolve();
    confirms.answer('stop');
    await stop;
    expect(
      calls('session_kill')
        .map((c) => (c.args as { id: string }).id)
        .sort(),
    ).toEqual([SHOP_CLAUDE, '0199a6b2-0000-7000-8000-000000000002']);
  });

  it('closing a tab without processes does not ask', async () => {
    await requestCloseTab('shop', 'shop-t3');
    expect(confirms.current).toBeNull();
    expect(layout.get('shop')!.tabs.map((t) => t.id)).not.toContain('shop-t3');
  });
});
