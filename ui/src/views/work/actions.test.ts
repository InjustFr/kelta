import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { dispatch } from '$lib/actions';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import type { RunClaim, SessionStatus, WorkItem } from '$lib/gen';
import { layout, projects, reviews, sessions, toasts, ui, work } from '$lib/stores';
import { menuView, type MenuItem } from '$lib/ui/Menu.svelte';

import '../reviews/actions';
import '../tickets/actions';
import './actions';
import { gitStatus, sessionInfo, workItem } from '$lib/gen/fixtures';

import { listScope } from '../../shell/scope.svelte';
import { blockedReason, workMenu } from './actions';
import { phaseNow } from './live';
import { globalContent } from './nav';
import { batch } from './batch.svelte';
import { REVIEW_READONLY } from './common';
import type { Phase, PhaseId, WorkActionId } from './phase';
import { selectTicket } from './selection.svelte';
import { onActions, REFRESH_EVENT, workUi } from './ui.svelte';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  toasts.clear();
  ui.sheets = [];
  selectTicket(null, null);
  await projects.load();
  await layout.load('shop');
});

describe('L9 actions', () => {
  it('tickets.open {project_id} shows the Tickets view scoped to that project, no tab (#409)', async () => {
    const tabs = layout.get('shop')?.tabs.length;
    expect(await dispatch('tickets.open', { project_id: 'shop' })).toBe(true);
    expect([ui.view, listScope.project]).toEqual(['tickets', 'shop']);
    expect(layout.get('shop')?.tabs.length).toBe(tabs);
    ui.view = null;
  });

  it('tickets.open and reviews.open without a project show the cross-project views', async () => {
    const tabs = layout.get('shop')?.tabs.length;
    await dispatch('tickets.open');
    expect(ui.view).toBe('tickets');
    await dispatch('reviews.open');
    expect(ui.view).toBe('reviews');
    expect(layout.get('shop')?.tabs.length).toBe(tabs);
    ui.view = null;
  });

  it('reviews.open from the leader key opens Reviews even where the chord would refresh (#417)', async () => {
    const pane = document.body.appendChild(document.createElement('div'));
    pane.tabIndex = 0;
    const refreshed = vi.fn((e: Event) => e.preventDefault());
    pane.addEventListener(REFRESH_EVENT, refreshed);
    pane.focus();
    ui.view = null;
    await dispatch('reviews.open', { chord: true });
    expect([refreshed.mock.calls.length, ui.view]).toEqual([1, null]);
    await dispatch('reviews.open'); // leader key `r`, palette
    expect([refreshed.mock.calls.length, ui.view]).toEqual([1, 'reviews']);
    pane.remove();
    ui.view = null;
  });

  it('tickets.board shows the Tickets view as the board of the active project (#409)', async () => {
    listScope.set(null);
    const tabs = layout.get('shop')?.tabs.length;
    await dispatch('tickets.board');
    expect([ui.view, listScope.project]).toEqual(['tickets', 'shop']);
    expect(globalContent('tickets')).toMatchObject({ mode: 'board' });
    expect(layout.get('shop')?.tabs.length).toBe(tabs);
    ui.view = null;
  });

  it('tickets.overview opens my tickets beside Reviews, once per project', async () => {
    ui.view = 'reviews';
    await dispatch('tickets.overview');
    expect(ui.view).toBeNull();
    const l = layout.get('shop');
    expect(l?.active_tab).toBe('overview');
    const root = l?.tabs.find((t) => t.id === 'overview')?.root;
    expect(root).toMatchObject({
      type: 'split',
      dir: 'row',
      children: [
        { type: 'pane', content: { kind: 'tickets', scope: { kind: 'project', id: 'shop' }, who: 'mine' } },
        { type: 'pane', content: { kind: 'reviews' } },
      ],
    });
    const tabs = l?.tabs.length;
    layout.update('shop', (x) => ({ ...x, active_tab: x.tabs[0]!.id }));
    await dispatch('tickets.overview');
    expect(layout.get('shop')?.tabs.length).toBe(tabs);
    expect(layout.get('shop')?.active_tab).toBe('overview');
  });

  it('reviews.open {project_id} shows the Reviews view scoped to that project, no tab (#409)', async () => {
    listScope.set(null);
    const tabs = layout.get('shop')?.tabs.length;
    await dispatch('reviews.open', { project_id: 'shop' });
    expect([ui.view, listScope.project]).toEqual(['reviews', 'shop']);
    expect(layout.get('shop')?.tabs.length).toBe(tabs);
    ui.view = null;
  });

  it('work.start without a selection explains what to do', async () => {
    await dispatch('work.start');
    expect(toasts.list.at(-1)?.toast.text).toMatch(/Select a ticket/);
    expect(mock.calls.some((c) => c.cmd === 'work_plan')).toBe(false);
  });

  it('work.start plans the selected ticket and opens the sheet', async () => {
    selectTicket({ account: 'jira-acme', key: 'SHOP-151', id: '10151' }, 'shop');
    await dispatch('work.start');
    expect(ui.sheet?.key).toBe('start_work');
    // the selection's project is the account's, no pick: the backend picks (SPEC §3.1)
    expect(mock.calls.filter((c) => c.cmd === 'work_plan').at(-1)?.args).toMatchObject({
      project_id: null,
      source: { kind: 'ticket', ticket: { key: 'SHOP-151' } },
    });
  });

  it('work.start with marked tickets opens the batch start instead (#141, #397)', async () => {
    selectTicket({ account: 'jira-acme', key: 'SHOP-151', id: '10151' }, 'shop');
    batch.toggle({ account: 'jira-acme', key: 'SHOP-160', id: '10160' }, 'shop');
    await dispatch('work.start');
    expect(ui.sheet?.key).toBe('start_batch');
    batch.clear();
    ui.closeSheet();
  });

  it('work.start accepts an explicit ticket', async () => {
    await dispatch('work.start', {
      ticket: { account: 'redmine-corp', key: '4590', id: '4590' },
      project_id: 'billing',
    });
    expect(ui.sheet?.key).toBe('start_work');
  });

  it('a failing plan is reported with a toast', async () => {
    mock.failNext('work_plan', { code: 'invalid_argument', message: 'no repo rule matches' });
    selectTicket({ account: 'jira-acme', key: 'SHOP-151', id: '10151' }, 'shop');
    await dispatch('work.start');
    expect(ui.sheet).toBeNull();
    expect(toasts.list.at(-1)?.toast.text).toMatch(/no repo rule matches/);
  });

  it('review items: push, rebase, fix and link are read-only in the menu and the work bar', () => {
    const review = { ...workItem, kind: 'review' as const, pr_url: 'https://x/pull/1' };
    for (const id of ['ship', 'rebase', 'fix', 'link', 'create_ticket'] as const)
      expect(blockedReason(id, review)).toBe(REVIEW_READONLY);
    expect(blockedReason('go_claude', review)).toBeNull();
    expect(blockedReason('ship', { ...review, kind: 'ticket' })).toBeNull();
  });
});

describe('work menu: what can run now, then Not available now (#405, #439)', () => {
  type Patch = Partial<WorkItem> & { pr?: true };
  // Per phase: the item (`pr`: with my open PR), its Claude (null: exited), the next step; then the listed
  // letters (⏎ the next step, `|` a new group: work, ticket, branch, PR) / the "Not available now" ones.
  // Only a `soon` action (⇧R) shows blocked: the others are ruled out for the phase and left out.
  const pr: Patch = { pr: true };
  const review: Patch = {
    kind: 'review',
    ticket: null,
    review: { account: 'github-acme', repo: 'acme/shop-api', number: 311 },
  };
  const run: RunClaim = {
    name: 'pass',
    run_id: 'r',
    session_id: null,
    phase: null,
    state: 'running',
    note: null,
    sha: null,
  };
  const rebase = {
    onto: 'origin/main',
    pre_head: 'a',
    remote_sha: null,
    conflicts: ['a.rs'],
    step: 1,
    total: 3,
  };
  const CASES: Record<PhaseId, [Patch, SessionStatus | null, WorkActionId | null, string]> = {
    asking: [{}, 'needs_input', 'go_claude', '⏎ | v V b P F | t | p r / R'],
    run: [{ run }, null, null, 'v V b g C F | t | p r / R'],
    failed: [
      { state: { kind: 'failed', step: 'setup', message: '1' } },
      null,
      'retry',
      '⏎ | v V s b g F | t | p r / R',
    ],
    starting: [{ state: { kind: 'starting' } }, null, null, 'v V b g F | t | p r / R'],
    queued: [{ state: { kind: 'queued', pos: 1 } }, null, 'start_now', '⏎ | v V b g F | t | p r / R'],
    missing: [{}, null, 'recreate', '⏎ | b g C F | t | p r / R'],
    rebase_stopped: [{ rebase } as Patch, 'done', 'resolve', '⏎ | v V b g P F | t | p r c a n / R'],
    needs_you: [{}, 'needs_input', 'go_claude', '⏎ | v V b P F | t | p r / R'],
    working: [{}, 'working', null, 'v V b g F | t | p r / R'],
    merged: [{ state: { kind: 'merged', detail: null } }, null, 'finish', '⏎ | v V b g C | t | p r / R'],
    closed: [{ state: { kind: 'pr_closed' } }, null, 'finish', '⏎ | v V b g C | t | p r / R'],
    to_review: [{ review_due: true }, 'done', 'review_delta', '⏎ | V R b g P F | t | p r / '],
    replied: [{ claude_replied: true }, 'done', 'go_claude', '⏎ | v V b P F | t | p r / R'],
    remote_new: [pr, 'done', 'rebase', '⏎ | v V b g P F | t | p | f o M d / R'],
    changes_requested: [pr, 'done', 'fix', '⏎ | v V b g P F | t | p r | o M d / R'],
    checks_failed: [pr, 'done', 'fix_checks', '⏎ | v V b g P F | t | p r | f o M d / R'],
    new_comments: [pr, 'done', 'fix', '⏎ | v V b g P F | t | p r | o M d / R'],
    conflicts: [pr, 'done', 'rebase', '⏎ | v V b g P F | t | p | f o M d / R'],
    rebased: [pr, 'done', 'ship', '⏎ | v V b g P F | t | r | f o M d / R'],
    unpushed: [pr, 'done', 'ship', '⏎ | v V b g P F | t | r | f o M d / R'],
    ready: [{}, 'done', 'ship', '⏎ | v V b g P F | t | r / R'],
    approved: [pr, 'done', 'open_pr', '⏎ | v V b g P F | t | p r | f M d / R'],
    draft: [pr, 'done', 'set_draft', '⏎ | v V b g P F | t | p r | f o M / R'],
    in_review: [pr, 'done', 'open_pr', '⏎ | v V b g P F | t | p r | f M d / R'],
    no_changes: [{}, 'done', 'go_claude', '⏎ | v V b P F | t | p r / R'],
    reviewing: [review, null, 'open_review', '⏎ | V b g C F / R'],
    reviewed: [review, null, 'finish', '⏎ | V b g C / R'],
    updated: [review, null, 'open_review', '⏎ | V b g C F / R'],
    elsewhere: [{}, null, null, 'v V b g C F | t | p r / R'],
  };

  const letters = (items: MenuItem[]) =>
    items.map((e) => `${e.separator ? '| ' : ''}${e.id === 'primary' ? '⏎' : e.key}`).join(' ');

  it('every phase of phase.ts', async () => {
    await reviews.load({ kind: 'all' }, 'authored', false);
    const mine = reviews.items({ kind: 'all' }, 'authored').find((r) => !r.review.assigned)!.review;
    for (const [id, [{ pr, ...patch }, status, primary, want]] of Object.entries(CASES)) {
      const item: WorkItem = {
        ...workItem,
        ...(pr ? { pr_url: mine.url, state: { kind: 'pr_open' } } : {}),
        ...patch,
      };
      sessions.upsert({ ...sessionInfo, status: status ?? 'done', lifecycle: status ? 'live' : 'exited' });
      const phase: Phase = {
        id: id as PhaseId,
        label: '',
        detail: '',
        lamp: 'none',
        section: 'in_flight',
        primary,
        primaryLabel: `${primary}`,
      };
      const { now, off } = menuView(workMenu(item, phase));
      expect(`${letters(now)} / ${off.map((e) => e.key).join(' ')}`, id).toBe(want);
      expect(off.every((e) => e.reason)).toBe(true);
      // Start Claude, Park and Merge when ready never sit under "Not available now" (#439).
      expect(
        off.filter((e) => ['C', 'P', 'M'].includes(e.key!)),
        id,
      ).toEqual([]);
    }
  });

  it('groups are headed by their object: Ticket KEY, Branch name with ↓n on Rebase, Pull request #n', async () => {
    await reviews.load({ kind: 'all' }, 'authored', false);
    const mine = reviews.items({ kind: 'all' }, 'authored').find((r) => !r.review.assigned)!.review;
    const item: WorkItem = { ...workItem, pr_url: mine.url, state: { kind: 'pr_open' } };
    work.git = { [item.id]: { ...gitStatus, behind: 3 } };
    const phase = { ...phaseNow(item), id: 'in_review' as const, primary: 'open_pr' as const };
    const { now } = menuView(workMenu(item, phase));
    expect(now.flatMap((e) => e.group ?? [])).toEqual([
      `Ticket ${item.ticket!.key}`,
      `Branch ${item.branch}`,
      `Pull request #${mine.ref.number}`,
    ]);
    expect(now.find((e) => e.id === 'rebase')?.detail).toBe('↓3');
    work.git = {};
  });
});

describe('⌘. opens the menu of what has the keys (#404)', () => {
  /** A focusable pane in the document whose `⌘.` handler logs and answers `opens`. */
  function pane(
    name: string,
    opens: boolean,
    log: string[],
    parent: HTMLElement = document.body,
  ): HTMLElement {
    const el = document.createElement('div');
    el.tabIndex = 0;
    parent.append(el);
    onActions(el, () => (log.push(name), opens));
    return el;
  }

  afterEach(() => {
    document.body.replaceChildren();
    ui.closeOverlay();
    ui.view = null;
    workUi.menu = null;
  });

  it('the focused detail opens its menu first; its list, the work menu and the palette are not asked', async () => {
    const log: string[] = [];
    const list = pane('list', true, log);
    pane('detail', true, log, list).focus();
    await dispatch('work.menu');
    expect(log).toEqual(['detail']);
    expect([ui.overlay, workUi.menu]).toEqual([null, null]);
  });

  it('a detail with nothing to offer hands it to its list: the selected row', async () => {
    const log: string[] = [];
    const list = pane('list', true, log);
    pane('detail', false, log, list).focus();
    await dispatch('work.menu');
    expect(log).toEqual(['detail', 'list']);
    expect(ui.overlay).toBeNull();
  });

  it('nothing selected and no work tab: the palette opens, never a toast', async () => {
    const log: string[] = [];
    ui.view = 'now'; // a view, not a work tab
    pane('list', false, log).focus();
    await dispatch('work.menu');
    expect(log).toEqual(['list']);
    expect(ui.overlay).toBe('palette');
    expect(toasts.list).toHaveLength(0);
  });

  it('a named work item (palette, rows) opens its work menu without asking the panes', async () => {
    const log: string[] = [];
    pane('list', true, log).focus();
    await work.load();
    const id = work.all[0]!.id;
    await dispatch('work.menu', { id });
    expect(log).toEqual([]);
    expect(workUi.menu).toBe(id);
  });
});
