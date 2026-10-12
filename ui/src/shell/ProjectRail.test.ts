import { fireEvent, render, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { LayoutNode } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { allPanes } from '$lib/layout';
import { layout, projects, sessions, tools, ui, work } from '$lib/stores';

import '../views/tools/actions';
import ProjectRail from './ProjectRail.svelte';

const SHOP_SHELL = '0199a6b2-0000-7000-8000-000000000003';
const LAZYDOCKER = '0199a6b2-0000-7000-8000-000000000004';
const SHOP_CLAUDE = '0199a6b2-0000-7000-8000-000000000001';
const BILLING_CLAUDE = '0199a6b2-0000-7000-8000-000000000005';

let m: MockControls;

/** Every open layout loaded (as the shell preloads them); `toolTab` leaves shop's Shell tab with lazydocker only. */
async function setup(toolTab = false, webInstance?: string) {
  const created = createMockTransport();
  m = created.controls;
  setTransport(created.transport);
  if (toolTab) {
    const tab = m.state.layouts.shop.tabs.find((t) => t.id === 'shop-t2')!;
    tab.root = allPanes(tab.root).find(
      (p) => p.content.kind === 'terminal' && p.content.session_id === LAZYDOCKER,
    ) as LayoutNode;
    if (webInstance && tab.root.type === 'pane')
      tab.root = { ...tab.root, content: { kind: 'web', tool_instance_id: webInstance } };
  }
  layout.byProject = {};
  tools.byProject = {};
  ui.view = null;
  await Promise.all([projects.load(), sessions.load(), work.load()]);
  await Promise.all(projects.list.filter((p) => p.open).map((p) => layout.ensure(p.id)));
  await tools.load(projects.activeId ?? 'home');
  return render(ProjectRail);
}

const bandRows = (r: { container: HTMLElement }, project: string) =>
  within(r.container.querySelector(`[data-testid="rail-band"][data-project-id="${project}"]`) as HTMLElement)
    .queryAllByTestId('rail-row')
    .map((e) => e.dataset.tabId);

describe('ProjectRail', () => {
  beforeEach(() => {
    projects.list = [];
  });

  it('one band per open project in rail order, one row per tab in tab order, with lamps and a lit row', async () => {
    const r = await setup();
    expect(r.getAllByTestId('rail-band').map((e) => e.dataset.projectId)).toEqual([
      'shop',
      'billing',
      'kelta-tools',
      'home',
    ]);
    expect(bandRows(r, 'shop')).toEqual(['shop-t1', 'shop-t2', 'shop-t3']);
    expect(bandRows(r, 'home')).toEqual(['home-t1', 'home-t2']);
    const billing = r.container.querySelector('[data-tab-id="billing-t1"]') as HTMLElement;
    expect(billing.dataset.attention).toBe('needs_input');
    expect(billing.classList).toContain('lit');
    expect(billing.querySelector('[data-attention="needs_input"]')).not.toBeNull();
    // The caption carries no lamp: its rows do.
    expect(r.getAllByTestId('rail-project')[0].querySelector('[data-attention]')).toBeNull();
  });

  it('a row click shows that tab of another project, also from a global view', async () => {
    const r = await setup();
    ui.view = 'now';
    await fireEvent.click(r.container.querySelector('[data-tab-id="billing-t2"]')!);
    expect(ui.view).toBeNull();
    expect(projects.activeId).toBe('billing');
    expect(layout.get('billing')?.active_tab).toBe('billing-t2');
  });

  it('a tool-only tab sits under Tools; a session in no pane is a dim row that reveals it', async () => {
    const r = await setup(true);
    expect(bandRows(r, 'shop')).toEqual(['shop-t1', 'shop-t3']);
    const tools = within(r.getByTestId('rail-tools'));
    expect(tools.getAllByTestId('rail-tool').map((e) => e.dataset.toolId)).toEqual(
      m.state.tools.map((t) => t.id),
    );
    const sub = tools.getByTestId('rail-tool-instance');
    expect(sub.dataset.toolId).toBe('lazydocker');
    expect(sub.textContent).toContain('└ Shop');

    const bg = r.getByTestId('rail-background');
    expect(bg.dataset.sessionId).toBe(SHOP_SHELL);
    await fireEvent.click(bg);
    await expect.poll(() => bandRows(r, 'shop').length).toBe(3);
    expect(r.queryByTestId('rail-background')).toBeNull();
  });

  it('an exited tool session is no tool instance: its tab is a closable row in its band', async () => {
    const r = await setup(true);
    sessions.apply({
      type: 'session.updated',
      session: { ...sessions.get(LAZYDOCKER)!, lifecycle: 'exited' },
    });
    await expect.poll(() => bandRows(r, 'shop')).toEqual(['shop-t1', 'shop-t2', 'shop-t3']);
    expect(within(r.getByTestId('rail-tools')).queryAllByTestId('rail-tool-instance')).toHaveLength(0);
  });

  it('a web tab whose instance is unknown (Kelta restarted) is a closable row, not a tool', async () => {
    const r = await setup(true, 'gone');
    expect(bandRows(r, 'shop')).toEqual(['shop-t1', 'shop-t2', 'shop-t3']);
    expect(within(r.getByTestId('rail-tools')).queryAllByTestId('rail-tool-instance')).toHaveLength(0);
  });

  it('tools: running PTY tools carry a lamp, web and external ones ↗, missing ones dimmed; a sub-line focuses its pane', async () => {
    const r = await setup();
    const row = (id: string) => r.container.querySelector(`[data-testid="rail-tool"][data-tool-id="${id}"]`)!;
    expect(row('lazydocker').getAttribute('data-attention')).toBe('activity');
    expect(row('lazygit').getAttribute('data-attention')).toBe('none');
    expect(row('sl-web').classList).toContain('missing');
    expect(row('sl-web').querySelector('.launch')).not.toBeNull();
    expect(row('lazygit').querySelector('.launch')).toBeNull();

    sessions.apply({
      type: 'session.updated',
      session: { ...sessions.get(LAZYDOCKER)!, lifecycle: 'exited' },
    });
    await expect.poll(() => row('lazydocker').getAttribute('data-attention')).toBe('none');
    sessions.apply({ type: 'session.updated', session: { ...sessions.get(LAZYDOCKER)!, lifecycle: 'live' } });

    ui.view = 'reviews';
    await expect.poll(() => r.queryAllByTestId('rail-tool-instance').length).toBe(1);
    await fireEvent.click(r.getByTestId('rail-tool-instance'));
    expect(ui.view).toBeNull();
    expect(projects.activeId).toBe('shop');
    const l = layout.get('shop')!;
    expect(l.active_tab).toBe('shop-t2');
    expect(l.tabs.find((t) => t.id === 'shop-t2')!.focused_pane).toBe(
      r.getByTestId('rail-tool-instance').dataset.paneId,
    );
  });

  it('workflows: each session current run with its phase or Needs input; a click opens its runs', async () => {
    const r = await setup();
    expect(r.queryByTestId('rail-workflows')).toBeNull();
    const base = {
      name: 'kelta-ticket-pass',
      status: 'running',
      phases: [{ title: 'Implement', agents: [{ id: 'a', label: 'impl', done: false }] }],
      duration_ms: null,
      total_tokens: null,
      total_tool_calls: null,
      result: null,
      logs: [],
    };
    sessions.apply({ type: 'workflow.updated', run: { ...base, run_id: 'wf_a', session_id: SHOP_CLAUDE } });
    sessions.apply({
      type: 'workflow.updated',
      run: { ...base, run_id: 'wf_b', session_id: BILLING_CLAUDE },
    });
    await expect.poll(() => r.queryAllByTestId('rail-workflow').length).toBe(2);
    const row = (id: string) => r.container.querySelector(`[data-run-id="${id}"]`) as HTMLElement;
    expect(row('wf_a').textContent).toContain('kelta-ticket-pass · Implement');
    expect(row('wf_a').dataset.attention).toBe('working');
    expect(row('wf_b').textContent).toContain('Needs input');
    expect(row('wf_b').dataset.attention).toBe('needs_input');

    await fireEvent.click(row('wf_a'));
    expect(ui.sheet).toEqual({ key: 'claude_runs', props: { sessionId: SHOP_CLAUDE } });
    ui.sheets = [];
  });

  it('tools: a tool without a Lucide icon shows a well-known icon, its 1-2 characters, else a monogram', async () => {
    const r = await setup();
    for (const [k, slot] of Object.entries(tools.byProject))
      tools.byProject = {
        ...tools.byProject,
        [k]: {
          ...slot,
          data:
            slot.data?.map((x) =>
              x.id === 'lazydocker' ? { ...x, icon: null } : x.id === 'lazygit' ? { ...x, icon: 'LG' } : x,
            ) ?? null,
        },
      };
    const row = (id: string) => r.container.querySelector(`[data-testid="rail-tool"][data-tool-id="${id}"]`)!;
    await expect.poll(() => row('lazygit').querySelector('.abbr')?.textContent).toBe('LG');
    expect(row('lazydocker').querySelector('.abbr')).toBeNull();
    expect(row('lazydocker').querySelector('use')?.getAttribute('href')).toBe('#i-container');
  });
});
