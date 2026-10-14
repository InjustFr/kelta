import { fireEvent, render, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { LayoutNode } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { allPanes } from '$lib/layout';
import { layout, projects, sessions, tools, ui, work } from '$lib/stores';

import { statusLabel } from './labels';
import { sidebar } from './sidebar.svelte';

import '../views/tools/actions';
import './actions';
import { maxLamp } from '../views/work/phase';
import { tabAttention } from './nav';
import ProjectRail from './ProjectRail.svelte';

const SHOP_SHELL = '0199a6b2-0000-7000-8000-000000000003';
const LAZYDOCKER = '0199a6b2-0000-7000-8000-000000000004';
const SHOP_CLAUDE = '0199a6b2-0000-7000-8000-000000000001';
const BILLING_CLAUDE = '0199a6b2-0000-7000-8000-000000000005';

let m: MockControls;

/** Every open layout loaded (as the shell preloads them); `toolTab` leaves shop's Shell tab with lazydocker only. */
async function setup(toolTab = false) {
  const created = createMockTransport();
  m = created.controls;
  setTransport(created.transport);
  if (toolTab) {
    const tab = m.state.layouts.shop.tabs.find((t) => t.id === 'shop-t2')!;
    tab.root = allPanes(tab.root).find(
      (p) => p.content.kind === 'terminal' && p.content.session_id === LAZYDOCKER,
    ) as LayoutNode;
  }
  layout.byProject = {};
  tools.byProject = {};
  ui.view = null;
  await Promise.all([projects.load(), sessions.load(), work.load()]);
  await Promise.all(projects.list.filter((p) => p.open).map((p) => layout.ensure(p.id)));
  await tools.load(projects.activeId ?? 'home');
  return render(ProjectRail);
}

const band = (r: { container: HTMLElement }, project: string) =>
  r.container.querySelector(`[data-testid="rail-band"][data-project-id="${project}"]`) as HTMLElement;
const bandRows = (r: { container: HTMLElement }, project: string) =>
  within(band(r, project))
    .queryAllByTestId('rail-row')
    .map((e) => e.dataset.tabId);
const moreRow = (r: { container: HTMLElement }, project: string) =>
  r.container.querySelector(`[data-testid="rail-more"][data-project-id="${project}"]`) as HTMLElement;
/** Opens a band's "n more" row (Home's "n shells"). */
const unfold = (r: { container: HTMLElement }, project: string) => fireEvent.click(moreRow(r, project));
const workIds = (r: { container: HTMLElement }, project: string) =>
  [...band(r, project).querySelectorAll<HTMLElement>('[data-work-id]')].map((e) =>
    e.dataset.workId!.slice(-4),
  );
const A = (n: string) => `0199a6b2-0000-7000-8000-00000000${n}`;

describe('ProjectRail', () => {
  beforeEach(() => {
    projects.list = [];
  });

  it('one band per open project in rail order: its work rows, the other tabs folded, lamps and a lit row', async () => {
    const r = await setup();
    // Home has no band (#435).
    expect(r.getAllByTestId('rail-band').map((e) => e.dataset.projectId)).toEqual([
      'shop',
      'billing',
      'kelta-tools',
    ]);
    expect(bandRows(r, 'shop')).toEqual(['shop-t1']);
    // Unfinished work in start order, tab or not, unless it waits on others (SHOP-120, #443); the caption counts them.
    expect(workIds(r, 'shop')).toEqual(['a008', 'a004', 'a001']);
    expect(within(band(r, 'shop')).getByTestId('rail-count').textContent).toBe('3');
    // Key, title without the key, stage word and ⌘N digit.
    const work = r.container.querySelector('[data-tab-id="shop-t1"]') as HTMLElement;
    expect(work.querySelector('.key')?.textContent).toBe('SHOP-142');
    expect(work.querySelector('.name')?.textContent).toBe('Rate-limit login');
    expect(work.querySelector('[data-testid="rail-stage"]')?.textContent).toBe('Claude');
    expect(r.container.querySelector('[data-tab-id="kt-t1"] .index')?.textContent).toBe('7');
    const more = within(band(r, 'shop')).getByTestId('rail-more');
    expect(more.textContent).toContain('2 more');
    expect(more.getAttribute('aria-expanded')).toBe('false');
    await unfold(r, 'shop');
    expect(bandRows(r, 'shop')).toEqual(['shop-t1', 'shop-t2', 'shop-t3']);
    expect(more.getAttribute('aria-expanded')).toBe('true');
    expect(r.container.querySelector('[data-tab-id="shop-t2"] .index')).toBeNull();
    // Home's tabs fold into "n shells" under Other Claude sessions.
    expect(moreRow(r, 'home').textContent).toContain('2 shells');
    await unfold(r, 'home');
    expect(
      within(r.getByTestId('rail-elsewhere'))
        .queryAllByTestId('rail-row')
        .map((e) => e.dataset.tabId),
    ).toEqual(['home-t1', 'home-t2']);
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
    await unfold(r, 'billing');
    await fireEvent.click(r.container.querySelector('[data-tab-id="billing-t2"]')!);
    expect(ui.view).toBeNull();
    expect(projects.activeId).toBe('billing');
    expect(layout.get('billing')?.active_tab).toBe('billing-t2');
  });

  it('no Tools band (#412): a tool-only tab is a band row; a session in no pane is a dim row that reveals it', async () => {
    const r = await setup(true);
    await unfold(r, 'shop');
    expect(r.queryByTestId('rail-tools')).toBeNull();
    expect(bandRows(r, 'shop')).toEqual(['shop-t1', 'shop-t2', 'shop-t3']);

    const bg = r.getByTestId('rail-background');
    expect(bg.dataset.sessionId).toBe(SHOP_SHELL);
    await fireEvent.click(bg);
    await expect.poll(() => bandRows(r, 'shop').length).toBe(4);
    expect(r.queryByTestId('rail-background')).toBeNull();
  });

  it('runs (#392): a claimed run on its ticket, the others on their session row with a count; a click splits the runs in', async () => {
    const r = await setup();
    expect(r.queryAllByTestId('rail-run')).toHaveLength(0);
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
    const emit = (run_id: string, session_id: string, name = base.name) =>
      sessions.apply({ type: 'workflow.updated', run: { ...base, run_id, name, session_id } });
    emit('wf_a', SHOP_CLAUDE);
    emit('wf_b', SHOP_CLAUDE, 'triage');
    emit('wf_c', BILLING_CLAUDE);
    const claim = (id: string, session_id: string, phase: string) => {
      const run = {
        name: base.name,
        run_id: 'r',
        session_id,
        phase,
        state: 'running' as const,
        note: null,
        sha: null,
      };
      work.apply({ type: 'work.updated', work: { ...work.get(id)!, run } });
    };
    claim(A('a004'), BILLING_CLAUDE, 'Pick'); // a ticket with no tab: on its row all the same (#435)
    claim(A('a003'), '0199a6b2-0000-7000-8000-000000000008', 'Review');

    await expect.poll(() => r.queryAllByTestId('rail-run').length).toBe(3);
    expect(r.queryByTestId('rail-workflows')).toBeNull();
    const sub = (sid: string) =>
      r.container.querySelector(`[data-testid="rail-run"][data-session-id="${sid}"]`);
    const shop = () => sub(SHOP_CLAUDE)!;
    expect(shop().textContent).toContain('kelta-ticket-pass · Implement +1');
    expect(sub('0199a6b2-0000-7000-8000-000000000008')?.textContent).toContain('kelta-ticket-pass · Review');
    // Its run is on its ticket's row, not on the billing tab.
    expect(
      sub(BILLING_CLAUDE)?.closest('li')?.previousElementSibling?.querySelector('[data-work-id]'),
    ).toHaveProperty('dataset.workId', A('a004'));

    const runsPanes = () =>
      allPanes(layout.get('shop')!.tabs.find((t) => t.id === 'shop-t1')!.root).filter(
        (p) => p.content.kind === 'claude_runs',
      );
    await fireEvent.click(shop());
    await expect.poll(() => runsPanes().length).toBe(1);
    const tab = layout.get('shop')!.tabs.find((t) => t.id === 'shop-t1')!;
    expect(runsPanes()[0]!.content).toEqual({ kind: 'claude_runs', session_id: SHOP_CLAUDE });
    expect(tab.focused_pane).toBe(runsPanes()[0]!.id);
    expect(layout.get('shop')!.active_tab).toBe('shop-t1');
    expect(ui.sheet).toBeNull();
    // Open again: focuses the same pane.
    layout.update('shop', (l) => ({ ...l, active_tab: 'shop-t2' }));
    await fireEvent.click(shop());
    await expect.poll(() => layout.get('shop')!.active_tab).toBe('shop-t1');
    expect(runsPanes()).toHaveLength(1);
  });

  it('views: Now, Tickets, Reviews, each with one count on its sub line and no red badge', async () => {
    const r = await setup();
    expect(r.getAllByTestId(/^rail-(inbox|tickets|reviews)$/).map((e) => e.dataset.testid)).toEqual([
      'rail-inbox',
      'rail-tickets',
      'rail-reviews',
    ]);
    await expect
      .poll(() => r.getByTestId('rail-reviews').title)
      .toMatch(/^Pull requests: \d+ asks? for your review$/);
    expect(r.getByTestId('rail-inbox').title).toMatch(/^Queue: \d+ your move$/);
    expect(r.getByTestId('rail-tickets').title).toMatch(/^Tickets: \d+ assigned to you$/);
  });

  it('n more: a folded tab in front marks it selected; its lamp is the highest of the folded tabs', async () => {
    const r = await setup();
    const more = () => within(band(r, 'billing')).getByTestId('rail-more');
    await fireEvent.click(r.container.querySelector('[data-tab-id="billing-t1"]')!);
    expect(more().classList).not.toContain('active');
    layout.update('billing', (l) => ({ ...l, active_tab: 'billing-t2' }));
    await expect.poll(() => more().classList.contains('active')).toBe(true);
    await unfold(r, 'billing');
    expect(more().classList).not.toContain('active'); // the row itself wears it once shown
    expect(r.container.querySelector('[data-tab-id="billing-t2"]')!.getAttribute('aria-current')).toBe(
      'page',
    );
    const shop = layout.get('shop')!.tabs.filter((t) => t.id !== 'shop-t1');
    expect(within(band(r, 'shop')).getByTestId('rail-more').dataset.attention).toBe(
      maxLamp(shop.map((t) => tabAttention(t))),
    );
  });

  it('a work item without a tab (#435): its lamp, key, title and stage; a click resumes it', async () => {
    const r = await setup();
    const row = () => band(r, 'shop').querySelector(`[data-work-id="${A('a004')}"]`) as HTMLElement;
    expect(row().dataset.testid).toBe('rail-resume');
    expect(row().querySelector('.key')?.textContent).toBe('SHOP-155');
    await expect.poll(() => row().querySelector('.name')?.textContent).toBe('Flaky test in cart service');
    expect(row().querySelector('[data-testid="rail-stage"]')?.textContent).toBeTruthy();
    expect(row().parentElement!.querySelector('.close')).toBeNull();
    await fireEvent.click(row());
    await expect.poll(() => m.calls.filter((c) => c.cmd === 'work_resume').length).toBe(1);
    await expect.poll(() => row().dataset.testid).toBe('rail-row');
    // The tab work_resume opened is titled by the key alone: the row keeps the ticket's title.
    expect(row().querySelector('.name')?.textContent).toBe('Flaky test in cart service');
  });

  it('a Claude session without a work item (#435): its own row, lit when it needs you; not in "n more"', async () => {
    const r = await setup();
    sessions.upsert({
      ...sessions.get(SHOP_CLAUDE)!,
      id: 'plain',
      name: 'claude 2',
      work_item_id: null,
      status: 'needs_input',
      attention: 'needs_input',
    });
    layout.open('shop', {
      content: { kind: 'terminal', session_id: 'plain' },
      placement: 'new_tab',
      focus: false,
      tab_title: 'claude 2',
      work_item_id: null,
    });
    const row = () => band(r, 'shop').querySelector('[data-session-id="plain"]') as HTMLElement;
    await expect.poll(() => row()).not.toBeNull();
    expect(row().classList).toContain('lit');
    expect(row().querySelector('.kind')?.textContent).toBe('✻');
    expect(row().querySelector('.name')?.textContent).toBe('claude 2');
    expect(row().querySelector('[data-testid="rail-stage"]')?.textContent).toBe(statusLabel('needs_input'));
    expect(moreRow(r, 'shop').textContent).toContain('2 more');
    expect(within(band(r, 'shop')).getByTestId('rail-count').textContent).toBe('5');
  });

  it('compact (#435): no "n more" rows and no unfolded tabs, only the work and Claude rows', async () => {
    const r = await setup();
    await unfold(r, 'shop');
    expect(bandRows(r, 'shop')).toEqual(['shop-t1', 'shop-t2', 'shop-t3']);
    sidebar.fold(true);
    try {
      await expect.poll(() => bandRows(r, 'shop')).toEqual(['shop-t1']);
      expect(moreRow(r, 'shop').closest('li')?.classList).toContain('fold-row');
    } finally {
      sidebar.fold(false);
    }
  });

  it('the band menu adds a project (#435): no Add button above the bands', async () => {
    const r = await setup();
    expect(r.queryByTestId('rail-add')).toBeNull();
    await fireEvent.click(within(band(r, 'shop')).getByRole('button', { name: 'Menu for Shop' }));
    await fireEvent.click(await r.findByRole('menuitem', { name: 'Add project' }));
    expect(ui.sheet?.key).toBe('project_new');
  });
});
