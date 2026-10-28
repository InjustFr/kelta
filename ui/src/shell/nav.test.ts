import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import { workItem } from '$lib/gen/fixtures';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { activeTab, closeTab } from '$lib/layout';
import { layout, projects, reviews, sessions, toasts, ui, work } from '$lib/stores';

import { zoneOf } from '../views/inbox/groups';
import { phaseNow } from '../views/work/live';
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
  backgroundRows,
  backgroundSessions,
  bandTabs,
  openRow,
  rowLamp,
  requestCloseTab,
  requestStopSession,
  revealSession,
  selectRow,
  splitFocused,
  tabAttention,
  toggleZoomFocused,
  viewKey,
  workRows,
  type RailRow,
} from './nav';

const SHOP_CLAUDE = '0199a6b2-0000-7000-8000-000000000001';
const BILLING_CLAUDE = '0199a6b2-0000-7000-8000-000000000005';

let mock: MockControls;
const A = (n: string) => `0199a6b2-0000-7000-8000-00000000${n}`;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  layout.byProject = {};
  reviews.lists = {};
  ui.view = null;
  toasts.clear();
  await Promise.all([projects.load(), sessions.load(), work.load()]);
  await layout.ensure('shop');
});

const calls = (cmd: string) => mock.calls.filter((c) => c.cmd === cmd);
/** A row's tab id, else the end of its work item or session id. */
const rowId = (r: RailRow) => r.tab?.id ?? (r.item ?? r.session)!.id.slice(-4);

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
    // Unfinished work items with a tab, in start order (#452); then the folded tabs.
    expect(railRows().map(rowId)).toEqual([
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
    expect(workRows().map(rowId)).toEqual(['shop-t1', 'billing-t1', 'kt-t1']);
    const at = () => [projects.activeId, currentTab()!.id];
    gotoRailRow(2);
    expect(at()).toEqual(['billing', 'billing-t1']);
    gotoRailRow(10); // no such work row: unchanged
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
    const order = () => workRows().map(rowId).slice(0, 2);
    expect(order()).toEqual(['shop-t4', 'shop-t1']);
    expect(railRows().map(rowId).slice(0, 4)).toEqual(['shop-t4', 'shop-t1', 'shop-t2', 'shop-t3']);
    work.apply({ type: 'work.updated', work: { ...work.get(started)!, state: { kind: 'pr_open' } } });
    expect(order()).toEqual(['shop-t4', 'shop-t1']);
    gotoRailRow(1);
    expect(currentTab()!.id).toBe('shop-t4');
  });

  it('no tab, no main row, whatever the phase (#452): live sessions go to Background, dormant ones nowhere', async () => {
    await layout.ensure('home');
    const rows = () => bandTabs('shop').work.map((r) => r.tab!.id);
    const bg = () => backgroundRows().map((r) => [r.session!.id.slice(-4), r.item?.id.slice(-4)]);
    // SHOP-155 is your move (dormant Claude), SHOP-120 waits on a reviewer: no tab, no row anywhere.
    expect(zoneOf(phaseNow(work.get(A('a007'))!).section)).toBe('waiting');
    expect(rows()).toEqual(['shop-t1']);
    expect(bg()).toEqual([]);
    layout.update('shop', (l) => ({
      ...l,
      tabs: [
        ...l.tabs,
        { ...l.tabs[2]!, id: 'shop-t4', title: 'SHOP-120 Payment SDK', work_item_id: A('a007') },
      ],
    }));
    expect(rows()).toEqual(['shop-t4', 'shop-t1']);
    layout.update('shop', (l) => closeTab(l, 'shop-t4'));
    // The reviewer requests changes (the mock's PR #305): its move, still no tab, still no row.
    await reviews.load({ kind: 'all' }, 'authored');
    expect(phaseNow(work.get(A('a007'))!).id).toBe('changes_requested');
    expect(rows()).toEqual(['shop-t1']);

    // Close tab, keep sessions: SHOP-142's Claude and nvim leave the band for Background, once each.
    const keep = requestCloseTab('shop', 'shop-t1');
    await Promise.resolve();
    confirms.answer('keep');
    await keep;
    expect(rows()).toEqual([]);
    expect(bg()).toEqual([
      ['0001', 'a001'],
      ['0002', 'a001'],
    ]);
    expect(railRows().some((r) => r.session)).toBe(false);
    // A click opens a tab on the session: the work row is back in its band.
    openRow(backgroundRows()[0]!);
    await expect.poll(() => rows().length).toBe(1);
    expect(bg()).toEqual([['0002', 'a001']]);
    // Close and stop sessions: gone from the sidebar.
    const stop = requestCloseTab('shop', rows()[0]!);
    await Promise.resolve();
    confirms.answer('stop');
    await stop;
    expect(rows()).toEqual([]);
    expect(calls('session_kill').map((c) => (c.args as { id: string }).id)).toEqual([SHOP_CLAUDE]);
    sessions.apply({ type: 'session.removed', id: SHOP_CLAUDE }); // the backend's answer to the kill
    expect(bg()).toEqual([['0002', 'a001']]);
  });

  it("a Background row's × stops its session after the confirm (#452)", async () => {
    const shell = sessions.get('0199a6b2-0000-7000-8000-000000000003')!;
    const cancel = requestStopSession(shell);
    await Promise.resolve();
    expect(confirms.current?.actions.map((a) => a.id)).toEqual(['stop']);
    confirms.answer(null);
    await cancel;
    expect(calls('session_kill')).toEqual([]);
    const stop = requestStopSession(shell);
    await Promise.resolve();
    confirms.answer('stop');
    await stop;
    expect(calls('session_kill').map((c) => (c.args as { id: string }).id)).toEqual([shell.id]);
  });

  it('a Claude session without a work item is a row of its own (#435), not a folded tab', async () => {
    const plain = (id: string, lifecycle: 'live' | 'exited') =>
      sessions.upsert({
        ...sessions.get(SHOP_CLAUDE)!,
        id,
        name: `claude ${id}`,
        work_item_id: null,
        lifecycle,
        attention: 'needs_input',
      });
    plain('plain-tab', 'live');
    plain('plain-bg', 'live');
    plain('plain-gone', 'exited');
    layout.open('shop', {
      content: { kind: 'terminal', session_id: 'plain-tab' },
      placement: 'new_tab',
      focus: false,
      tab_title: 'claude',
      work_item_id: null,
    });
    const band = bandTabs('shop');
    expect(band.claude.map((r) => [r.session!.id, !!r.tab])).toEqual([['plain-tab', true]]);
    expect(band.more.map((t) => t.id)).toEqual(['shop-t2', 'shop-t3']);
    expect(rowLamp(band.claude[0]!)).toBe('needs_input');
    // Live with no tab: a Background row (#452), its lamp live; a click shows it in a new tab.
    const bg = backgroundRows().find((r) => r.session!.id === 'plain-bg')!;
    expect([bg.tab, bg.item, rowLamp(bg)]).toEqual([null, undefined, 'needs_input']);
    expect(backgroundRows().map((r) => r.session!.id)).not.toContain('plain-gone');
    openRow(bg);
    await expect.poll(() => bandTabs('shop').claude.map((r) => r.session!.id)).toContain('plain-bg');
    expect(backgroundRows().map((r) => r.session!.id)).not.toContain('plain-bg');
  });

  it("a run's Claude in its work item's tab is that item's row, not a Claude row too (#491)", () => {
    work.upsert({ ...workItem, id: 'run-item', project_id: 'shop', state: { kind: 'active' } });
    sessions.upsert({ ...sessions.get(SHOP_CLAUDE)!, id: 'run-s', work_item_id: null, lifecycle: 'live' });
    layout.open('shop', {
      content: { kind: 'terminal', session_id: 'run-s' },
      placement: 'new_tab',
      focus: false,
      tab_title: 'SHOP-142',
      work_item_id: 'run-item',
    });
    const band = bandTabs('shop');
    expect(band.claude.map((r) => r.session!.id)).not.toContain('run-s');
    expect(band.work.some((r) => r.item!.id === 'run-item')).toBe(true);
  });

  it('a live Claude whose work item is finished, tab closed, is a Background row without its item', () => {
    const item = work.get('0199a6b2-0000-7000-8000-00000000a004')!;
    work.apply({ type: 'work.updated', work: { ...item, state: { kind: 'finished' } } as typeof item });
    sessions.upsert({
      ...sessions.get(SHOP_CLAUDE)!,
      id: 'orphan',
      work_item_id: item.id,
      lifecycle: 'live',
      attention: 'needs_input',
    });
    expect(backgroundSessions('shop').map((s) => s.id)).toContain('orphan');
    expect(backgroundRows().find((r) => r.session!.id === 'orphan')?.item).toBeUndefined();
    expect(bandTabs('shop').claude.map((r) => r.session!.id)).not.toContain('orphan');
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

  it('the new-session sheet defaults to the template started last', async () => {
    localStorage.removeItem('kelta.newSession.template.v1');
    const first = render(NewSessionSheet, { onclose: () => {} });
    const select = within(await screen.findByTestId('new-session')).getAllByRole('combobox')[0]!;
    expect((select as HTMLSelectElement).value).toBe('claude+editor');
    await fireEvent.change(select, { target: { value: 'claude' } });
    await fireEvent.click(screen.getByTestId('new-session-start'));
    await waitFor(() => expect(calls('session_spawn_template')).toHaveLength(1));
    first.unmount();
    render(NewSessionSheet, { onclose: () => {} });
    const again = within(await screen.findByTestId('new-session')).getAllByRole('combobox')[0]!;
    expect((again as HTMLSelectElement).value).toBe('claude');
    localStorage.removeItem('kelta.newSession.template.v1');
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

  it('the close key leaves Settings and touches no hidden pane', async () => {
    const pane = currentTab()!.focused_pane;
    ui.settings = { section: null };
    await closeFocusedPane();
    expect(ui.settings).toBeNull();
    expect(confirms.current).toBeNull();
    expect(currentTab()!.focused_pane).toBe(pane);
    expect(layout.get('shop')!.tabs).toHaveLength(3);
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
