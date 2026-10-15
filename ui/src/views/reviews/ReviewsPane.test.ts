import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import * as ipc from '$lib/ipc/commands';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { findContent } from '$lib/layout';
import type { JsonValue, ReviewKind, Settings } from '$lib/gen';
import { GLOBAL, layout, projects, reviews, settings, tickets, toasts, ui } from '$lib/stores';

import { openPr } from '../tickets/prs';
import TicketsPane from '../tickets/TicketsPane.svelte';
import { globalContent } from '../work/nav';
import { takeKeyRequest } from '../settings/actions';
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
  it('only an exact key opens: a key no tracker knows says so', async () => {
    mountReviews();
    // SHOP-150 is linked by a review but no ticket has that key (SHOP-15x do not count).
    await fireEvent.click(await screen.findByRole('button', { name: 'Open SHOP-150' }));
    await waitFor(() => expect(toasts.list.at(-1)?.toast.text).toMatch(/SHOP-150 is not in any/));
    const l = layout.get('shop');
    expect(l && findContent(l, (c) => c.kind === 'ticket_detail')).toBeFalsy();
  });
});

describe('ReviewsPane row actions (#397)', () => {
  it('the selected row has Open review and a menu of every key; an action that cannot run says why', async () => {
    const { container } = mountReviews();
    const acts = await waitFor(() => {
      const el = container.querySelector<HTMLElement>('.row.selected .acts');
      if (!el?.firstElementChild) throw new Error('no row actions yet');
      return el;
    });
    expect(container.querySelectorAll('.acts [role="button"]')).toHaveLength(1);
    await fireEvent.click(within(acts).getByRole('button', { name: 'More actions' }));
    const menu = await screen.findByRole('menu', { name: 'More actions' });
    const entry = (name: string) => within(menu).getByRole('menuitem', { name: new RegExp(`^${name}`) });
    expect(entry('Review locally').querySelector('kbd')?.textContent).toBe('S');
    // For me lists other people's PRs: Nudge is mine only.
    // Not available now (#405): last, the reason under the label.
    expect(entry('Nudge reviewers').getAttribute('aria-disabled')).toBe('true');
    expect(entry('Nudge reviewers').querySelector('.reason')?.textContent).toBe(
      'Only on your own pull requests',
    );
    expect(within(menu).getAllByRole('menuitem').at(-1)?.getAttribute('aria-disabled')).toBe('true');
    expect(within(menu).getByText(/^Not available now \(\d\)$/)).toBeTruthy();
    // Open in own pane opens the PR in its own pane.
    await fireEvent.click(entry('Open in own pane'));
    await waitFor(() =>
      expect(findContent(layout.get('shop')!, (c) => c.kind === 'review_detail')).toBeTruthy(),
    );
  });
});

describe('ReviewsPane keys (#417)', () => {
  it("c comments on someone's PR; v without a checkout says how to get one", async () => {
    ui.sheets = [];
    mountReviews();
    await waitFor(() => expect(document.querySelector('.row.selected')).not.toBeNull());
    const pane = screen.getByTestId('reviews-pane');
    await fireEvent.keyDown(pane, { key: 'c' });
    expect(ui.sheets.map((s) => s.key)).toEqual(['reviews.comment']);
    expect(sheetRegistry['reviews.comment']).toBeTypeOf('function');
    await fireEvent.keyDown(pane, { key: 'v' });
    await waitFor(() => expect(toasts.list.at(-1)?.toast.text).toMatch(/review it locally first \(s\)/));
  });
});

describe('ReviewsPane next step column (#447)', () => {
  it('a row not mine has an Open review button; Yours shows the step as text', async () => {
    mountGlobal();
    await waitFor(() => expect(document.querySelectorAll('.row').length).toBeGreaterThan(0));
    const all = [...document.querySelectorAll<HTMLElement>('button.row')];
    expect(all.length).toBeGreaterThan(0);
    for (const row of all) {
      const dim = row.classList.contains('dim');
      expect(row.querySelector('[data-next]') === null).toBe(dim);
      if (!dim) expect(within(row).getByRole('button', { name: 'Open review' })).toBeTruthy();
      else expect(row.querySelector('.next [role="button"]')).toBeNull();
    }
    expect(document.querySelector('.row.dim [data-next-text]')?.textContent).toMatch(/\S/);
    // A click selects that row and moves the keys into its detail, as Enter does.
    const target = all.find((r) => !r.classList.contains('dim'))!;
    await fireEvent.click(within(target).getByRole('button', { name: 'Open review' }));
    expect(document.querySelector('.row.selected')).toBe(target);
    await waitFor(() => expect(document.activeElement).toBe(detail()));
  });
});

describe('ReviewsPane filters', () => {
  it('a filter that hides every PR says which filters are on; Clear all brings them back (#370)', async () => {
    const { container } = mountReviews();
    await waitFor(() => expect(container.querySelectorAll('.row').length).toBeGreaterThan(0));
    await fireEvent.input(screen.getByLabelText('Filter pull requests'), { target: { value: 'zzzz' } });
    expect(await screen.findByText(/^No match: .*"zzzz"/)).toBeTruthy();
    await fireEvent.click(screen.getAllByRole('button', { name: 'Clear all' })[0]!);
    await waitFor(() => expect(container.querySelectorAll('.row').length).toBeGreaterThan(0));
    expect(container.querySelector('[data-filter]')).toBeNull();
  });
});

describe('ReviewsPane in the rail Reviews view', () => {
  it("Shift+Enter opens the review in the PR's project and leaves the view", async () => {
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
    await waitFor(() => expect(document.querySelector('.row.selected')).not.toBeNull());
    await fireEvent.keyDown(screen.getByTestId('reviews-pane'), { key: 'Enter', shiftKey: true });
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
const selectedKey = () => document.querySelector('.row.selected')?.getAttribute('data-key');

describe('ReviewsPane brief beside the list', () => {
  const ready = () => waitFor(() => expect(selectedKey()).toBeTruthy());

  it('shows the selected PR; Enter focuses it, a approves from it, Esc returns', async () => {
    mountReviews();
    await ready();
    const key = selectedKey()!;
    await waitFor(() => expect(detail()?.getAttribute('aria-label')).toBe(`Review ${key.split(':')[1]}`));
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
    // #440: Space hides and shows the detail; Enter shows it again too.
    await fireEvent.keyDown(pane(), { key: ' ' });
    expect(detail()).toBeNull();
    await fireEvent.keyDown(pane(), { key: ' ' });
    await waitFor(() => expect(detail()).not.toBeNull());
  });

  it('holding j through the list loads the detail of the row it stops on, not every row passed', async () => {
    mountReviews();
    await ready();
    const first = selectedKey();
    const gets = () => mock.calls.filter((c) => c.cmd === 'review_get').length;
    const before = gets();
    for (let i = 0; i < 10; i++) await fireEvent.keyDown(pane(), { key: 'j' });
    expect(selectedKey()).not.toBe(first);
    await new Promise((r) => setTimeout(r, 400));
    expect(gets() - before).toBeLessThanOrEqual(2);
    await screen.findByTestId('head-sha');
  });

  it('Shift+Enter opens the standalone pane', async () => {
    mountReviews();
    await ready();
    await fireEvent.keyDown(pane(), { key: 'Enter', shiftKey: true });
    await waitFor(() => expect(JSON.stringify(layout.get('shop'))).toContain('"review_detail"'));
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
    // The selected PR's brief carries its linked tickets.
    const linked = mock.state.reviews.find((r) => r.review.linked_tickets.includes(key))!.review.ref;
    const row = await waitFor(() => {
      const el = document.querySelector<HTMLElement>(`.row[data-key$="${linked.repo}#${linked.number}"]`);
      if (!el) throw new Error('no row yet');
      return el;
    });
    await fireEvent.click(row);
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

describe('ReviewsPane nudge (#240)', () => {
  it('the nudge menu opens on my PRs only', async () => {
    mountReviews();
    await waitFor(() => expect(selectedKey()).toBeTruthy());
    const asked = [...document.querySelectorAll<HTMLElement>('button.row')].find((r) =>
      r.dataset.key?.endsWith('acme/shop-api#311'),
    )!;
    await fireEvent.click(asked);
    await fireEvent.keyDown(pane(), { key: 'n' });
    expect(screen.queryByRole('menu', { name: 'Nudge reviewers' })).toBeNull();
    const mine = [...document.querySelectorAll<HTMLElement>('button.row')].find((r) =>
      r.dataset.key?.endsWith('acme/shop-api#305'),
    )!;
    await fireEvent.click(mine);
    await fireEvent.keyDown(pane(), { key: 'n' });
    expect(await screen.findByRole('menu', { name: 'Nudge reviewers' })).toBeTruthy();
  });
});

describe('ReviewsPane sections and Team (#241, #401)', () => {
  beforeEach(async () => {
    settings.effective = {};
    await Promise.all([projects.load(), settings.load()]);
  });
  const headers = () =>
    [...document.querySelectorAll('[data-section]')].map((h) => h.textContent?.replace(/\s+/g, ' ').trim());
  const keys = () => [...document.querySelectorAll('.row[data-key]')].map((r) => r.getAttribute('data-key'));
  const press = (key: string) => fireEvent.keyDown(pane(), { key });
  const keysOf = (kind: ReviewKind) =>
    mock.state.reviews
      .filter((r) => r.review.kind === kind)
      .map((r) => `${r.review.ref.account}:${r.review.ref.repo}#${r.review.ref.number}`);
  const mountScope = (scope: { kind: 'all' } | { kind: 'project'; id: string } = { kind: 'all' }) =>
    render(ReviewsPane, {
      props: {
        projectId: 'shop',
        tabId: 'tab-1',
        paneId: 'p',
        content: { kind: 'reviews', scope },
        visible: true,
        focused: true,
      },
    });
  const teamLoaded = () =>
    mock.calls.some((c) => c.cmd === 'review_list' && (c.args as { kind: string }).kind === 'team');

  it("asked of me, the team's ready PRs, its drafts folded, then mine; each PR once", async () => {
    mountGlobal();
    await waitFor(() =>
      expect(headers()).toEqual([
        `Asked of you ${keysOf('review_requested').length}`,
        'Team, needs a reviewer 2',
        'Team drafts 1',
        `Yours, as reviewers see them ${keysOf('authored').length}`,
      ]),
    );
    expect(teamLoaded()).toBe(true);
    const k = (n: number) => keys().find((x) => x?.endsWith(`#${n}`));
    // #322 merges into #320: it follows it, an elbow deeper; the draft #324 waits behind its fold.
    expect(keys().indexOf(k(322)!)).toBe(keys().indexOf(k(320)!) + 1);
    expect(document.querySelector(`.row[data-key="${k(322)}"] .elbow`)).not.toBeNull();
    expect(document.querySelector(`.row[data-key="${k(320)}"]`)?.textContent).toContain(
      'base of a stack of 2',
    );
    expect(k(324)).toBeUndefined();
    expect(new Set(keys()).size).toBe(keys().length);
    await fireEvent.click(screen.getByRole('button', { name: /^Team drafts/ }));
    await waitFor(() => expect(k(324)).toBeTruthy());
    await press('Home');
    for (let i = 0; i < keys().length + 2; i++) {
      await press('j');
      expect(selectedKey()).toBeTruthy();
    }
    await fireEvent.click(screen.getByRole('button', { name: 'Settings for Team drafts' }));
    expect(takeKeyRequest()).toBe('reviews.include_drafts');
    await fireEvent.click(screen.getByRole('button', { name: 'Settings for Team, needs a reviewer' }));
    expect(takeKeyRequest()).toBe('reviews.include_team_requests');
    ui.settings = null;
  });

  it('a project no source feeds lists no Team', async () => {
    (mock.state.settings.value as unknown as Settings).sources = [];
    settings.effective = {};
    await settings.load();
    mountScope({ kind: 'project', id: 'shop' });
    await waitFor(() => expect(headers()[0]).toMatch(/^Asked of you/));
    expect(headers().some((h) => h?.startsWith('Team'))).toBe(false);
    expect(teamLoaded()).toBe(false);
  });

  it('sources that omit projects, routes or views still offer Team in the project it feeds (#281)', async () => {
    const value: JsonValue[] = [
      { id: 'other', label: 'Other', tracker: { account: 'jira-acme' } },
      { id: 'fed', label: 'Fed', projects: ['shop'], tracker: { account: 'jira-acme' } },
    ];
    settings.accept(null, await ipc.settingsSet({ layer: 'global', path: 'sources', value }));
    mountScope({ kind: 'project', id: 'shop' });
    await waitFor(() => expect(teamLoaded()).toBe(true));
  });

  it('p from a ticket lands on Team when the PR is only there', async () => {
    const base = mock.state.reviews.find((r) => r.project_ids.includes('shop'))!;
    const ref = { ...base.review.ref, number: 999 };
    mock.state.reviews.push({ ...base, review: { ...base.review, ref, kind: 'team', draft: false } });
    await Promise.all(
      (['review_requested', 'authored', 'team'] as const).map((k) => reviews.load({ kind: 'all' }, k)),
    );
    const pr = mock.state.tickets.find((t) => t.ticket.ref.key === '4567')!.prs[0]!;
    ui.view = 'tickets';
    openPr({ ...pr, account: ref.account, repo: ref.repo, number: 999 }, 'shop');
    expect(ui.view).toBe('reviews');
    mountGlobal();
    await waitFor(() => expect(selectedKey()).toBe(`${ref.account}:${ref.repo}#999`));
  });
});

describe('ReviewsPane workflows (#355)', () => {
  it('w runs a PR workflow on the row; a second run opens the running one', async () => {
    await settings.load();
    mountReviews();
    await waitFor(() => expect(selectedKey()).toBeTruthy());
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
