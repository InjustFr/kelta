import { fireEvent, render, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { LayoutNode } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { allPanes } from '$lib/layout';
import { layout, projects, sessions, tools, ui, work } from '$lib/stores';

import { confirms } from './confirm.svelte';
import { sessionTitle, statusLabel } from './labels';
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
    // Unfinished work whose tab is open (#452), whatever its phase; the caption counts them.
    expect(workIds(r, 'shop')).toEqual(['a001']);
    expect(within(band(r, 'shop')).getByTestId('rail-count').textContent).toBe('1');
    expect(r.queryByTestId('rail-background-section')).toBeNull();
    // Key, title without the key, stage word; the ⌘N digit only while the modifier is held (#451).
    const work = r.container.querySelector('[data-tab-id="shop-t1"]') as HTMLElement;
    expect(work.querySelector('.key')?.textContent).toBe('SHOP-142');
    expect(work.querySelector('.name')?.textContent).toBe('Rate-limit login');
    expect(work.querySelector('[data-testid="rail-stage"]')?.textContent).toBe('Claude');
    const kt = () => r.container.querySelector('[data-tab-id="kt-t1"]') as HTMLElement;
    const digit = () => kt().querySelector('.index')?.textContent;
    expect(digit()).toBeUndefined();
    await fireEvent.keyDown(window, { key: 'Meta', metaKey: true, ctrlKey: true });
    // The digit and the tooltip's chord name the same row index.
    expect(kt().title).toMatch(new RegExp(`\\((⌘|Ctrl\\+)${digit()}\\)$`));
    await fireEvent.keyUp(window, { key: 'Meta' });
    expect(digit()).toBeUndefined();
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

  it('no Tools band (#412): a tool-only tab is a band row; a shell in no pane is a Background row, not in "n more"', async () => {
    const r = await setup(true);
    await unfold(r, 'shop');
    expect(r.queryByTestId('rail-tools')).toBeNull();
    expect(bandRows(r, 'shop')).toEqual(['shop-t1', 'shop-t2', 'shop-t3']);
    expect(within(band(r, 'shop')).queryByTestId('rail-background')).toBeNull();

    const bg = r.getByTestId('rail-background');
    expect(bg.dataset.sessionId).toBe(SHOP_SHELL);
    expect(bg.title).toMatch(/^Shop › /);
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
    claim(A('a002'), BILLING_CLAUDE, 'Pick');
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
    ).toHaveProperty('dataset.workId', A('a002'));

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

  it('Background (#452): a closed tab whose sessions keep running; a click brings it back, × stops it', async () => {
    const r = await setup();
    const keep = r.container
      .querySelector('[data-tab-id="shop-t1"]')!
      .parentElement!.querySelector('.close')!;
    await fireEvent.click(keep);
    confirms.answer('keep');
    const section = () => r.queryByTestId('rail-background-section');
    await expect.poll(() => section()).not.toBeNull();
    expect(section()!.querySelector('h2')?.textContent?.replace(/\s+/g, ' ').trim()).toBe('Background 2');
    expect(workIds(r, 'shop')).toEqual([]);
    const rows = () => within(section()!).queryAllByTestId('rail-background');
    expect(rows().map((e) => [e.dataset.sessionId, e.dataset.workId])).toEqual([
      [SHOP_CLAUDE, A('a001')],
      ['0199a6b2-0000-7000-8000-000000000002', A('a001')],
    ]);
    // Like a work row, dimmed, its project named; no ⌘N digit, no duplicate under "n more".
    const first = rows()[0]!;
    expect(first.classList).toContain('background');
    expect(first.querySelector('.key')?.textContent).toBe('SHOP-142');
    expect(first.title).toBe(`Shop › SHOP-142 ${first.querySelector('.name')?.textContent}`);
    // The item's nvim names itself in the stage slot, so the two rows differ.
    expect(rows().map((e) => e.querySelector('[data-testid="rail-stage"]')?.textContent)).toEqual([
      'Claude',
      sessionTitle(sessions.get(rows()[1]!.dataset.sessionId!)!),
    ]);
    await fireEvent.keyDown(window, { key: 'Meta', metaKey: true, ctrlKey: true });
    expect(section()!.querySelector('.index')).toBeNull();
    await fireEvent.keyUp(window, { key: 'Meta' });
    await unfold(r, 'shop');
    expect(within(band(r, 'shop')).queryAllByTestId('rail-background')).toHaveLength(0);

    // A click opens a tab on the session: the work row is back in its band.
    await fireEvent.click(first);
    await expect.poll(() => workIds(r, 'shop')).toEqual(['a001']);
    expect(rows()).toHaveLength(1);
    // × asks, then stops the session: the row and the section leave.
    await fireEvent.click(rows()[0]!.parentElement!.querySelector('.close')!);
    expect(confirms.current?.actions.map((a) => a.id)).toEqual(['stop']);
    confirms.answer('stop');
    await expect.poll(() => m.calls.filter((c) => c.cmd === 'session_kill').length).toBe(1);
    sessions.apply({ type: 'session.removed', id: rows()[0]!.dataset.sessionId! }); // the backend's answer
    await expect.poll(() => section()).toBeNull();
  });

  it('a Claude session without a work item (#435): its own row, lit when it needs you; not in "n more"', async () => {
    const r = await setup();
    sessions.upsert({
      ...sessions.get(SHOP_CLAUDE)!,
      id: 'plain',
      name: 'claude 2',
      title: null,
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
    // #450: the conversation title names the row, minus its status glyph; spinner frames change nothing.
    for (const title of ['✳ Kelta UI and IDE UX', '⠂ Kelta UI and IDE UX', '⠐ Kelta UI and IDE UX']) {
      sessions.upsert({ ...sessions.get('plain')!, title });
      await expect.poll(() => row().querySelector('.name')?.textContent).toBe('Kelta UI and IDE UX');
    }
    expect(moreRow(r, 'shop').textContent).toContain('2 more');
    expect(within(band(r, 'shop')).getByTestId('rail-count').textContent).toBe('2');
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
