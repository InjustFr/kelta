import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { findContent } from '$lib/layout';
import { GLOBAL, layout, projects, reviews, tickets, toasts, ui } from '$lib/stores';

import { openPr } from '../tickets/prs';
import TicketsPane from '../tickets/TicketsPane.svelte';
import { globalContent } from '../work/nav';
import ReviewsPane from './ReviewsPane.svelte';

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
  it('a linked ticket badge opens that ticket without opening the review', async () => {
    mountReviews();
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
