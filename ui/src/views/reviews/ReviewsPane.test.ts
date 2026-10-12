import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import * as ipc from '$lib/ipc/commands';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { findContent } from '$lib/layout';
import type { JsonValue, ReviewKind, Settings } from '$lib/gen';
import { GLOBAL, layout, projects, reviews, settings, tickets, toasts, ui } from '$lib/stores';

import { openPr } from '../tickets/prs';
import TicketsPane from '../tickets/TicketsPane.svelte';
import { globalContent, loadGlobalLayout } from '../work/nav';
import ReviewsPane from './ReviewsPane.svelte';
import { sheetRegistry } from '$app/registry';

let mock: MockControls;

beforeEach(() => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  reviews.lists = {};
  layout.byProject = {};
  toasts.clear();
  ui.select = null;
});
// Unmount, not only empty the DOM: a leftover rail pane would take the next test's `ui.select`.
afterEach(cleanup);

function mountReviews() {
  return render(ReviewsPane, {
    props: {
      projectId: 'shop',
      tabId: 't',
      paneId: 'p',
      content: { kind: 'reviews', scope: { kind: 'all' } },
      visible: true,
      focused: true,
    },
  });
}

describe('ReviewsPane linked tickets', () => {
  it('rows are two-line until the Compact switch is on (#383)', async () => {
    mountReviews();
    await screen.findAllByRole('button', { name: /^Open SHOP-/ });
    expect(document.querySelector('.k-row.two')).not.toBeNull();
    await fireEvent.click(screen.getByRole('switch', { name: 'Compact' }));
    await waitFor(() => expect(document.querySelector('.k-row.two')).toBeNull());
    expect(document.querySelector('.k-row')).not.toBeNull();
  });

  it('a linked ticket badge opens that ticket without opening the review', async () => {
    mountReviews();
    await fireEvent.keyDown(await screen.findByTestId('reviews-pane'), { key: '3' }); // Mine
    const badge = await screen.findByRole('button', { name: 'Open SHOP-120' });
    await fireEvent.dblClick(badge); // would open the review if it reached the row
    await fireEvent.click(badge);
    await waitFor(() => {
      const l = layout.get('shop')!;
      expect(findContent(l, (c) => c.kind === 'ticket_detail' && c.ticket.key === 'SHOP-120')).toBeTruthy();
    });
    expect(findContent(layout.get('shop')!, (c) => c.kind === 'review_detail')).toBeFalsy();
  });

  it('only an exact key opens: a key no tracker knows says so', async () => {
    mountReviews();
    // SHOP-150 is linked by a review but no ticket has that key (SHOP-15x do not count).
    await fireEvent.click(await screen.findByRole('button', { name: 'Open SHOP-150' }));
    await waitFor(() => expect(toasts.list.at(-1)?.toast.text).toMatch(/SHOP-150 is not in any/));
    const l = layout.get('shop');
    expect(l && findContent(l, (c) => c.kind === 'ticket_detail')).toBeFalsy();
  });
});

describe('ReviewsPane filters', () => {
  it('a filter that hides every PR says which filters are on; Clear all brings them back (#370)', async () => {
    const { container } = mountReviews();
    await waitFor(() => expect(container.querySelectorAll('.k-row').length).toBeGreaterThan(0));
    await fireEvent.click(screen.getByRole('switch', { name: 'Drafts' }));
    await fireEvent.input(screen.getByLabelText('Filter reviews'), { target: { value: 'zzzz' } });
    expect(await screen.findByText('No match: 2 filters on ("zzzz", No drafts).')).toBeTruthy();
    await fireEvent.click(screen.getAllByRole('button', { name: 'Clear all' })[0]!);
    await waitFor(() => expect(container.querySelectorAll('.k-row').length).toBeGreaterThan(0));
    expect(container.querySelector('[data-filter]')).toBeNull();
  });
});

describe('ReviewsPane in the rail Reviews view', () => {
  it("Enter opens the review in the PR's project and leaves the view", async () => {
    await projects.load();
    ui.view = 'reviews';
    render(ReviewsPane, {
      props: {
        projectId: GLOBAL,
        tabId: GLOBAL,
        paneId: 'reviews',
        content: { kind: 'reviews', scope: { kind: 'all' } },
        visible: true,
        focused: true,
      },
    });
    await screen.findAllByRole('button', { name: /^Open SHOP-/ });
    await fireEvent.keyDown(screen.getByTestId('reviews-pane'), { key: 'Enter' });
    await waitFor(() => expect(ui.view).toBeNull());
    const pid = projects.activeId!;
    expect(pid).not.toBe(GLOBAL);
    await waitFor(() =>
      expect(findContent(layout.get(pid)!, (c) => c.kind === 'review_detail')).toBeTruthy(),
    );
  });
});

const mountGlobal = () =>
  render(ReviewsPane, {
    props: {
      projectId: GLOBAL,
      tabId: GLOBAL,
      paneId: 'reviews',
      content: { kind: 'reviews', scope: { kind: 'all' } },
      visible: true,
      focused: true,
    },
  });
const pane = () => screen.getByTestId('reviews-pane');
const detail = () => screen.queryByTestId('review-detail');
const selectedKey = () => document.querySelector('.k-row.selected')?.getAttribute('data-key');

describe('ReviewsPane split view', () => {
  let width = 1000;
  beforeEach(() => {
    width = 1000;
    Object.defineProperty(HTMLElement.prototype, 'clientWidth', { configurable: true, get: () => width });
    return () => void delete (HTMLElement.prototype as { clientWidth?: number }).clientWidth;
  });

  it('shows the selected PR; Space toggles it, Enter focuses it, a approves from it, Esc returns', async () => {
    mountReviews();
    await screen.findAllByRole('button', { name: /^Open SHOP-/ });
    const key = selectedKey()!;
    await waitFor(() => expect(detail()?.getAttribute('aria-label')).toBe(`Review ${key.split(':')[1]}`));
    await fireEvent.keyDown(pane(), { key: ' ' });
    expect(detail()).toBeNull();
    await fireEvent.keyDown(pane(), { key: ' ' });
    await fireEvent.keyDown(pane(), { key: 'Enter' });
    await waitFor(() => expect(document.activeElement).toBe(detail()));
    await screen.findByTestId('head-sha');
    await fireEvent.keyDown(detail()!, { key: 'a' });
    await waitFor(() => expect(mock.calls.filter((c) => c.cmd === 'review_approve')).toHaveLength(1));
    await fireEvent.keyDown(detail()!, { key: 'j' }); // the detail's keys stay in the detail
    expect(selectedKey()).toBe(key);
    await fireEvent.keyDown(detail()!, { key: 'Escape' });
    expect(document.activeElement).toBe(pane());
    expect(JSON.stringify(layout.get('shop') ?? null)).not.toContain('"review_detail"');
  });

  it('holding j through the list loads the detail of the row it stops on, not every row passed', async () => {
    mountReviews();
    await screen.findAllByRole('button', { name: /^Open SHOP-/ });
    const first = selectedKey();
    const gets = () => mock.calls.filter((c) => c.cmd === 'review_get').length;
    const before = gets();
    for (let i = 0; i < 10; i++) await fireEvent.keyDown(pane(), { key: 'j' });
    expect(selectedKey()).not.toBe(first);
    await new Promise((r) => setTimeout(r, 400));
    expect(gets() - before).toBeLessThanOrEqual(2);
    await screen.findByTestId('head-sha');
  });

  it('Shift+Enter opens the standalone pane; a narrow pane has no column', async () => {
    const wide = mountReviews();
    await screen.findAllByRole('button', { name: /^Open SHOP-/ });
    await fireEvent.keyDown(pane(), { key: 'Enter', shiftKey: true });
    await waitFor(() => expect(JSON.stringify(layout.get('shop'))).toContain('"review_detail"'));
    wide.unmount();
    width = 600;
    mountReviews();
    await screen.findAllByRole('button', { name: /^Open SHOP-/ });
    expect(detail()).toBeNull();
  });
});

describe('ReviewsPane cross links (rail views)', () => {
  beforeEach(async () => {
    localStorage.removeItem('kelta.view.tickets');
    await projects.load();
  });

  it('p on a ticket lands on its PR in Reviews; a PR in neither list still opens in its project', async () => {
    await reviews.load({ kind: 'all' }, 'review_requested');
    await reviews.load({ kind: 'all' }, 'authored');
    const pr = mock.state.tickets.find((t) => t.ticket.ref.key === '4567')!.prs[0]!;
    ui.view = 'tickets';
    openPr(pr, 'billing');
    expect(ui.view).toBe('reviews');
    mountGlobal();
    await waitFor(() => expect(selectedKey()).toBe(`${pr.account}:${pr.repo}#${pr.number}`));
    expect(ui.select).toBeNull();

    ui.view = 'tickets';
    openPr({ ...pr, number: 4242 }, 'billing');
    await waitFor(() => expect(ui.view).toBeNull());
    await waitFor(() =>
      expect(findContent(layout.get('billing')!, (c) => c.kind === 'review_detail')).toBeTruthy(),
    );
  });

  it("a PR's linked ticket loaded in the Tickets view lands there", async () => {
    const tv = globalContent('tickets');
    if (tv.kind !== 'tickets') throw new Error('tickets');
    await tickets.load(tv.scope, tv.view_id, false, tv.who);
    const loaded = tickets.items(tv.scope, tv.view_id, tv.who).map((i) => i.ticket.ref.key);
    ui.view = 'reviews';
    mountGlobal();
    const key = ['SHOP-120', 'SHOP-142', '4567'].find((k) => loaded.includes(k))!;
    expect(key).toBeTruthy();
    await fireEvent.click((await screen.findAllByRole('button', { name: `Open ${key}` }))[0]!);
    expect(ui.view).toBe('tickets');
    const want = ui.select!.key;
    expect(want).toMatch(new RegExp(`:${key}$`));
    cleanup();
    render(TicketsPane, {
      props: {
        projectId: GLOBAL,
        tabId: GLOBAL,
        paneId: 'tickets',
        content: tv,
        visible: true,
        focused: true,
      },
    });
    await waitFor(() =>
      expect(
        document.querySelector('[data-testid="tickets-pane"] .selected[data-key]')?.getAttribute('data-key'),
      ).toBe(want),
    );
    expect(ui.select).toBeNull();
  });
});

describe('ReviewsPane waiting chips (#240)', () => {
  it('chips every row; amber past the SLA on my PRs only; the nudge menu opens on mine only', async () => {
    const asked = { why: 'review', since: '2020-01-01T00:00:00Z', matched: true } as const;
    const authored = mock.state.reviews.find((r) => r.review.kind === 'authored' && r.waiting.length > 0)!;
    authored.waiting = [{ ...asked, person: 'gh:anna', name: 'Anna' }];
    const request = mock.state.reviews.find((r) => r.review.kind === 'review_requested')!;
    request.waiting = [{ ...asked, person: 'gh:bob', name: 'Bob', why: 'author', since: null }];
    mountReviews();
    const theirs = await screen.findByText('waiting on Bob (author)');
    expect(theirs.className).not.toMatch(/warn/);
    await fireEvent.click(theirs.closest('.k-row')!);
    await fireEvent.keyDown(screen.getByTestId('reviews-pane'), { key: 'n' });
    expect(screen.queryByRole('menu', { name: 'Nudge reviewers' })).toBeNull();
    await fireEvent.keyDown(screen.getByTestId('reviews-pane'), { key: '3' });
    const mine = await screen.findByText(/waiting on Anna/);
    expect(mine.className).toMatch(/warn/);
    await fireEvent.click(mine.closest('.k-row')!);
    await fireEvent.keyDown(screen.getByTestId('reviews-pane'), { key: 'n' });
    expect(await screen.findByRole('menu', { name: 'Nudge reviewers' })).toBeTruthy();
  });
});

describe('ReviewsPane tabs and Team (#241)', () => {
  beforeEach(async () => {
    localStorage.removeItem('kelta.view.reviews');
    settings.effective = {};
    await Promise.all([projects.load(), settings.load()]);
  });
  const tabs = () => screen.getAllByRole('tab').map((t) => t.textContent?.trim());
  const keys = () =>
    [...document.querySelectorAll('.k-row[data-key]')].map((r) => r.getAttribute('data-key'));
  const press = (key: string) => fireEvent.keyDown(pane(), { key });
  const keysOf = (kind: ReviewKind) =>
    mock.state.reviews
      .filter((r) => r.review.kind === kind)
      .map((r) => `${r.review.ref.account}:${r.review.ref.repo}#${r.review.ref.number}`);
  const mountTab = (
    tab: ReviewKind | null,
    scope: { kind: 'all' } | { kind: 'project'; id: string } = { kind: 'all' },
  ) =>
    render(ReviewsPane, {
      props: {
        projectId: 'shop',
        tabId: 'tab-1',
        paneId: 'p',
        content: { kind: 'reviews', scope, tab },
        visible: true,
        focused: true,
      },
    });

  it('For me / Team / Mine with counts, switched by 1 2 3; the rail view saves the tab and polls it', async () => {
    await loadGlobalLayout();
    ui.view = 'reviews';
    mountGlobal();
    await waitFor(() => expect(tabs()).toEqual(['For me 5', 'Team', 'Mine 3']));
    expect(keys().sort()).toEqual(keysOf('review_requested').sort());
    await press('3');
    await waitFor(() => expect(keys().sort()).toEqual(keysOf('authored').sort()));
    expect(
      mock.calls.some((c) => c.cmd === 'review_list' && (c.args as { kind: string }).kind === 'team'),
    ).toBe(false);
    await press('2');
    await waitFor(() => expect(tabs()).toEqual(['For me 5', 'Team 7', 'Mine 3']));
    expect(globalContent('reviews')).toMatchObject({ tab: 'team' });
    await layout.flush(GLOBAL);
    expect(JSON.stringify(mock.state.layouts[GLOBAL])).toContain('"tab":"team"');
  });

  it('a project pane keeps its tab; a project no source feeds has no Team tab', async () => {
    layout.byProject = {
      shop: {
        project_id: 'shop',
        active_tab: 'tab-1',
        rev: 0,
        tabs: [
          {
            id: 'tab-1',
            title: 'Reviews',
            work_item_id: null,
            focused_pane: 'p',
            zoomed_pane: null,
            root: {
              type: 'pane',
              id: 'p',
              content: { kind: 'reviews', scope: { kind: 'project', id: 'shop' } },
            },
          },
        ],
      },
    } as unknown as typeof layout.byProject;
    const fed = mountTab(null, { kind: 'project', id: 'shop' });
    await waitFor(() => expect(tabs()[1]).toBe('Team'));
    await press('2');
    const root = layout.get('shop')!.tabs[0]!.root;
    expect(root.type === 'pane' && root.content).toMatchObject({ kind: 'reviews', tab: 'team' });
    fed.unmount();
    (mock.state.settings.value as unknown as Settings).sources = [];
    settings.effective = {};
    await settings.load();
    mountTab('team', { kind: 'project', id: 'shop' });
    await waitFor(() => expect(tabs().map((t) => t?.split(' ')[0])).toEqual(['For', 'Mine']));
    expect(screen.getByRole('tab', { selected: true }).textContent?.trim()).toMatch(/^For me/);
  });

  it('sources that omit projects, routes or views still offer Team in the project it feeds (#281)', async () => {
    const value: JsonValue[] = [
      { id: 'other', label: 'Other', tracker: { account: 'jira-acme' } },
      { id: 'fed', label: 'Fed', projects: ['shop'], tracker: { account: 'jira-acme' } },
    ];
    settings.accept(null, await ipc.settingsSet({ layer: 'global', path: 'sources', value }));
    mountTab(null, { kind: 'project', id: 'shop' });
    await waitFor(() => expect(tabs()[1]).toBe('Team'));
  });

  it('Team rows read "you" on my PRs; G groups them by first ticket, No ticket last; j k skip headers', async () => {
    mountTab('team');
    await waitFor(() => expect(keys()).toHaveLength(7));
    const mine = keysOf('authored')[0]!;
    const row = (k: string) => document.querySelector(`.k-row[data-key="${k}"]`)!;
    expect(row(mine).textContent).toContain('you');
    expect(row(keysOf('review_requested')[0]!).textContent).toMatch(/@\S+/);
    expect(document.querySelector('.k-group')).toBeNull();
    await press('G');
    const headers = () => [...document.querySelectorAll('.k-group')].map((h) => h.textContent?.trim() ?? '');
    await waitFor(() => expect(headers().at(-1)).toMatch(/^No ticket/));
    expect(headers().find((h) => h.startsWith('SHOP-142'))).toMatch(/2$/);
    expect(headers()).toContainEqual(expect.stringMatching(/^ada\/kelta-tools#12/));
    await press('Home');
    for (let i = 0; i < 8; i++) {
      await press('j');
      expect(selectedKey()).toBeTruthy();
    }
    await press('G');
    await waitFor(() => expect(document.querySelector('.k-group')).toBeNull());
  });

  it('p from a ticket lands on Team when the PR is only there', async () => {
    const base = mock.state.reviews.find((r) => r.project_ids.includes('shop'))!;
    const ref = { ...base.review.ref, number: 999 };
    mock.state.reviews.push({ ...base, review: { ...base.review, ref, kind: 'team' } });
    await Promise.all(
      (['review_requested', 'authored', 'team'] as const).map((k) => reviews.load({ kind: 'all' }, k)),
    );
    const pr = mock.state.tickets.find((t) => t.ticket.ref.key === '4567')!.prs[0]!;
    ui.view = 'tickets';
    openPr({ ...pr, account: ref.account, repo: ref.repo, number: 999 }, 'shop');
    expect(ui.view).toBe('reviews');
    mountGlobal();
    await waitFor(() => expect(selectedKey()).toBe(`${ref.account}:${ref.repo}#999`));
    expect(screen.getByRole('tab', { selected: true }).textContent?.trim()).toMatch(/^Team/);
  });
});

describe('ReviewsPane workflows (#355)', () => {
  it('w runs a PR workflow on the row; a second run opens the running one', async () => {
    await settings.load();
    mountReviews();
    await screen.findAllByRole('button', { name: /^Open SHOP-/ });
    await fireEvent.keyDown(screen.getByTestId('reviews-pane'), { key: 'w' });
    await waitFor(() => expect(ui.sheet?.key).toBe('workflow_run'));
    const { review, workflows } = ui.sheet!.props as {
      review: { repo: string; number: number };
      workflows: { name: string }[];
    };
    expect(workflows.map((w) => w.name)).toEqual(['describe-pr']);
    Element.prototype.scrollIntoView ??= () => {}; // not in jsdom
    const Sheet = (await sheetRegistry.workflow_run()).default;
    const props = { ...ui.sheet!.props, onclose: () => {} };
    const pick = async () => {
      const sheet = render(Sheet, { props });
      const name = `Run workflow on ${review.repo}#${review.number}`;
      await fireEvent.keyDown(screen.getByRole('textbox', { name }), { key: 'Enter' });
      sheet.unmount();
    };
    await pick();
    await waitFor(() =>
      expect(toasts.list.at(-1)?.toast.text).toBe(`Describe PR started on ${review.repo}#${review.number}`),
    );
    expect(mock.calls.find((c) => c.cmd === 'workflow_run_on')?.args).toEqual({
      project_id: 'shop',
      workflow: 'describe-pr',
      review,
    });
    await pick();
    await waitFor(() =>
      expect(toasts.list.at(-1)?.toast.text).toBe('A workflow is already running here: opened it'),
    );
    expect(mock.state.sessions.filter((s) => s.name?.startsWith('Describe PR'))).toHaveLength(1);
  });
});
