import { beforeEach, describe, expect, it } from 'vitest';

import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { activeTab } from '$lib/layout';
import { layout, projects, sessions, toasts, ui, work } from '$lib/stores';

import { confirms } from './confirm.svelte';
import {
  activateProject,
  attentionNext,
  closeFocusedPane,
  currentTab,
  cycleProject,
  cycleTab,
  focusedSessionId,
  gotoProjectIndex,
  openContent,
  railProjects,
  requestCloseTab,
  revealSession,
  splitFocused,
  tabAttention,
  toggleZoomFocused,
} from './nav';

const SHOP_CLAUDE = '0199a6b2-0000-7000-8000-000000000001';
const BILLING_CLAUDE = '0199a6b2-0000-7000-8000-000000000005';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  layout.byProject = {};
  ui.inboxActive = false;
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

  it('goto and cycling follow the rail order and wrap around', async () => {
    gotoProjectIndex(2);
    expect(projects.activeId).toBe('billing');
    gotoProjectIndex(9); // no such project: unchanged
    expect(projects.activeId).toBe('billing');
    cycleProject(1);
    expect(projects.activeId).toBe('kelta-tools');
    cycleProject(1);
    expect(projects.activeId).toBe('home');
    cycleProject(1);
    expect(projects.activeId).toBe('shop');
    cycleProject(-1);
    expect(projects.activeId).toBe('home');
  });

  it('leaves the inbox when a project is activated', async () => {
    ui.inboxActive = true;
    await activateProject('shop');
    expect(ui.inboxActive).toBe(false);
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

  it('opens a background session in a new tab', async () => {
    closeFocusedPane(); // shop-p1 (Claude) leaves the layout but keeps running
    expect(layout.get('shop')!.tabs[0]!.root.type).toBe('pane');
    const tabs = layout.get('shop')!.tabs.length;
    await revealSession(SHOP_CLAUDE);
    expect(layout.get('shop')!.tabs).toHaveLength(tabs + 1);
    expect(focusedSessionId()).toBe(SHOP_CLAUDE);
    expect(calls('session_kill')).toEqual([]);
  });

  it('attention.next goes to the session needing input in another project', async () => {
    await attentionNext();
    expect(projects.activeId).toBe('billing');
    expect(focusedSessionId()).toBe(BILLING_CLAUDE);
  });

  it('attention.next tells when nothing needs input', async () => {
    for (const s of sessions.all) sessions.upsert({ ...s, status: 'running', attention: 'none' });
    await attentionNext();
    expect(toasts.list.at(-1)?.toast.text).toBe('No session needs input');
    expect(projects.activeId).toBe('shop');
  });

  it('computes tab attention from the sessions shown in the tab', async () => {
    const tab = layout.get('shop')!.tabs[0]!;
    expect(tabAttention(tab)).toBe('working'); // the shop Claude session is working (attention: activity)
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

  it('toggles zoom on the focused pane', () => {
    toggleZoomFocused();
    expect(currentTab()!.zoomed_pane).toBe(currentTab()!.focused_pane);
    toggleZoomFocused();
    expect(currentTab()!.zoomed_pane).toBeNull();
  });

  it('closing the last pane of a tab closes the tab, never a process', () => {
    const tabs = layout.get('shop')!.tabs.length;
    closeFocusedPane();
    closeFocusedPane();
    expect(layout.get('shop')!.tabs).toHaveLength(tabs - 1);
    expect(calls('session_kill')).toEqual([]);
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
