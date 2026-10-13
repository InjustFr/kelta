// Flow 1 on the IPC mock (FLOW §4.1): Now → Up next → start work → Claude stops with changes →
// Ready for review row → Enter opens the delta zoomed in the work tab. Also the work menu from a terminal.
import { expect, test, type Page } from '@playwright/test';

import { activeProject, boot, callsOf, dispatch } from '../shell/helpers';

const now = (page: Page) => page.getByTestId('inbox-pane');
const selected = (page: Page) => now(page).locator('.row[aria-current="true"]');

/** Moves the Now selection with j until `rowId` is selected. */
async function select(page: Page, rowId: string): Promise<void> {
  for (let i = 0; i < 30; i += 1) {
    if ((await selected(page).getAttribute('data-row')) === rowId) return;
    await page.keyboard.press('j');
  }
  throw new Error(`row ${rowId} not found`);
}

test('Flow 1: ticket, Claude, review the diff', async ({ page }) => {
  // Two-line rows (#383): a taller window keeps every Now section rendered.
  await page.setViewportSize({ width: 1280, height: 1200 });
  await boot(page);
  await page.keyboard.press('Control+0');
  await expect(now(page)).toBeVisible();
  await expect(now(page).locator('[data-zone="yours"]')).toBeVisible();

  // Up next → s → start sheet → Ctrl+Enter: the work tab opens focused and Now closes.
  await select(page, 't:jira-acme:SHOP-151');
  await page.keyboard.press('s');
  const sheet = page.getByRole('dialog', { name: /Start work on SHOP-151/ });
  await expect(sheet).toBeVisible();
  await page.keyboard.press('Enter'); // Start anyway: the mock has unfinished work (#354)
  await expect(sheet.getByLabel('Branch', { exact: true })).toBeVisible();
  await page.keyboard.press('Control+Enter');
  await expect(sheet).toBeHidden();
  await expect(now(page)).toBeHidden();
  await expect.poll(() => activeProject(page)).toBe('shop');
  const header = page.getByTestId('work-header');
  await expect(page.getByTestId('context-key')).toHaveText('SHOP-151');

  // Claude stops with changes: review_due (the backend's Stop hook) and a fresh diffstat.
  await page.evaluate(() => {
    const mock = window.__keltaMock!;
    const w = mock.state.work.find((x) => x.ticket?.key === 'SHOP-151')!;
    mock.state.git[w.id] = {
      ahead: 1,
      behind: 0,
      dirty: true,
      unpushed: true,
      files: 2,
      insertions: 30,
      deletions: 4,
      missing: false,
    };
    const claude = mock.state.sessions.find((s) => s.id === w.session_ids[0])!;
    Object.assign(claude, { status: 'done', claude: { ...claude.claude!, preview: 'Tax lines added.' } });
    mock.emit({ type: 'session.updated', session: structuredClone(claude) });
    w.review_due = true;
    mock.emit({ type: 'work.updated', work: structuredClone(w) });
  });
  await expect(header).toHaveAttribute('data-phase', 'to_review');

  // Now: the item's next step is Review changes, with Claude's last message; Enter opens the delta.
  await page.keyboard.press('Control+0');
  const id = await page.evaluate(
    () => window.__kelta!.stores.work.all.find((w) => w.ticket?.key === 'SHOP-151')!.id,
  );
  await expect(now(page).locator(`[data-row="w:${id}"] [data-next]`)).toHaveText('Review changes');
  await select(page, `w:${id}`);
  await expect(selected(page)).toContainText('Tax lines added.');
  await page.keyboard.press('Enter');
  await expect(now(page)).toBeHidden();
  await expect.poll(async () => (await callsOf(page, 'work_diff')).length).toBe(1);
  // The diff pane is zoomed in the work tab: the only visible pane.
  await expect(page.locator('[data-testid="pane"]:visible')).toHaveCount(1);
  await expect(header).toBeVisible();
});

test('the work menu opens from a terminal with its chord; letters never move', async ({ page }) => {
  await boot(page);
  // The shop tab hosts SHOP-142 (Claude + nvim); focus is in a terminal.
  await expect(page.getByTestId('work-header')).toBeVisible();
  await page.locator('[data-testid="terminal-pane"] .xterm').first().click();
  await page.keyboard.press('Control+Period');
  const menu = page.getByRole('menu', { name: 'Work' });
  await expect(menu).toBeVisible();
  await expect(menu.getByRole('menuitem')).toHaveCount(22);
  // SHOP-142's Claude runs: Start Claude waits for an item without one.
  await expect(menu.getByRole('menuitem', { name: /Start Claude/ })).toHaveAttribute(
    'title',
    'Claude is already running',
  );
  await expect(menu.getByRole('menuitem', { name: /Continue rebase/ })).toHaveAttribute(
    'title',
    'Only while a rebase is stopped',
  );
  await page.keyboard.press('Escape');
  await expect(menu).toBeHidden();
});

test('Start Claude gives a work item whose Claude is gone a new one in its tab', async ({ page }) => {
  await boot(page);
  await expect(page.getByTestId('work-header')).toBeVisible();
  const panes = page.locator('[data-testid="terminal-pane"]:visible');
  const before = await panes.count();
  await page.evaluate(() => {
    const mock = window.__keltaMock!;
    const w = mock.state.work.find((x) => x.ticket?.key === 'SHOP-142')!;
    const claude = mock.state.sessions.find((s) => w.session_ids.includes(s.id) && s.kind.type === 'claude')!;
    Object.assign(claude, { lifecycle: 'exited', status: 'exited' });
    mock.emit({ type: 'session.updated', session: structuredClone(claude) });
  });
  await page.locator('[data-testid="terminal-pane"] .xterm').first().click();
  await page.keyboard.press('Control+Period');
  await page
    .getByRole('menu', { name: 'Work' })
    .getByRole('menuitem', { name: /Start Claude/ })
    .click();
  await expect.poll(async () => (await callsOf(page, 'work_start_claude')).length).toBe(1);
  await expect(panes).toHaveCount(before + 1);
});

test('Now open refreshes review items: a moved head reads Updated since your review', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 1200 });
  await boot(page);
  // A local checkout of #309, which I approved at its current head.
  const id = await page.evaluate(() => {
    const mock = window.__keltaMock!;
    const pr = mock.state.reviews.find((r) => r.review.ref.number === 309)!;
    const w = structuredClone(mock.state.work[0]!);
    Object.assign(w, {
      id: 'review-309',
      kind: 'review',
      ticket: null,
      review: pr.review.ref,
      branch: 'kelta/pr-309',
      pr_url: null,
      session_ids: [],
      state: { kind: 'active' },
      review_due: false,
      claude_replied: false,
    });
    mock.state.work.push(w);
    mock.emit({ type: 'work.updated', work: structuredClone(w) });
    return w.id;
  });
  await page.keyboard.press('Control+0');
  const row = now(page).locator(`[data-row="w:${id}"]`);
  await expect(row).toContainText('Reviewed');
  // #101's head moved after my review: back in Review requests.
  await expect(now(page).locator('[data-row^="r:"][data-row$="#101"]')).toContainText(
    'Updated since your review',
  );

  // Someone pushes to #309 while Now is closed; opening Now asks the host again (`review_get`).
  await page.evaluate(() => {
    window.__kelta!.stores.ui.view = null;
    const pr = window.__keltaMock!.state.reviews.find((r) => r.review.ref.number === 309)!;
    pr.review.head_sha = '3'.repeat(40);
  });
  await expect(now(page)).toBeHidden();
  const before = (await callsOf(page, 'review_get')).length;
  await page.keyboard.press('Control+0');
  await expect(row).toContainText('Updated since your review');
  const gets = (await callsOf(page, 'review_get')).slice(before);
  expect(gets.some((c) => (c.args as { review: { number: number } }).review.number === 309)).toBe(true);
});

test('a Claude session Kelta did not start: Elsewhere, Claude needs you, Mod+J, its ticket row (#345)', async ({
  page,
}) => {
  await boot(page);
  const badge = page.getByTestId('inbox-badge');
  const waiting = async (): Promise<number> =>
    (await badge.count()) ? Number(await badge.textContent()) : 0;
  const before = await waiting();
  const emit = (status: string) =>
    page.evaluate((s) => {
      const session = {
        id: 'u1',
        cwd: '/src/shop',
        title: 'Fix checkout',
        status: s as 'working',
        pid: 42,
        branch: 'feat/SHOP-151',
        ticket: { account: 'jira-acme', key: 'SHOP-151', id: 'SHOP-151' },
        preview: null,
        updated_at: '2020-01-01T00:00:00Z',
      };
      // What core lists too (Now's refresh reloads it).
      window.__keltaMock!.state.outside = [session];
      window.__keltaMock!.emit({ type: 'outside.updated', session });
    }, status);

  await emit('working');
  const rail = page.getByTestId('rail-outside');
  await expect(rail).toHaveAttribute('data-attention', 'working');
  await expect(rail).toContainText('Fix checkout');
  expect(await waiting()).toBe(before);

  await emit('needs_input');
  await expect(rail).toHaveAttribute('data-attention', 'needs_input');
  await expect.poll(waiting).toBe(before + 1);
  await page.keyboard.press('Control+0');
  await expect(now(page).locator('[data-zone="yours"]')).toBeVisible();
  await expect(now(page).locator('[data-row="o:u1"]')).toContainText('Claude needs you, outside Kelta');
  await dispatch(page, 'attention.next');
  await expect(page.getByTestId('jump-hud')).toContainText(/^1\/\d+ · needs input · Fix checkout/);
  // #346: no window control in the mock, so the jump degrades to Peek with where it runs.
  const peekBox = page.getByTestId('peek');
  await expect(page.getByTestId('peek-where')).toContainText('it runs in Terminal');
  await expect(peekBox).toContainText('/src/shop');
  await page.keyboard.press('Escape');
  await expect(peekBox).toBeHidden();
  await rail.click();
  await expect(peekBox).toHaveAttribute('data-outside-id', 'u1');
  await page.keyboard.press('Escape');
  expect((await callsOf(page, 'outside_jump')).map((c) => c.args)).toEqual([{ id: 'u1' }, { id: 'u1' }]);

  await dispatch(page, 'tickets.open', { project_id: 'shop' });
  await page.getByRole('tab', { name: 'List' }).click();
  // The Tickets row puts it with Claude, my move (#400), and says why in its tooltip.
  const ticket = page.locator('[data-key="jira-acme:SHOP-151"]');
  await expect(ticket.locator('[data-mark]')).toHaveAttribute('data-mark', 'you');
  await expect(ticket).toHaveAttribute('title', /Claude needs you, outside Kelta/);

  await page.evaluate(() => {
    window.__keltaMock!.state.outside = [];
    window.__keltaMock!.emit({ type: 'outside.removed', id: 'u1' });
  });
  await expect(rail).toHaveCount(0);
  await expect.poll(waiting).toBe(before);
});

test('at 936px the brief stays beside the list, not under it', async ({ page }) => {
  await page.setViewportSize({ width: 936, height: 1000 });
  await boot(page);
  await page.keyboard.press('Control+0');
  const list = await now(page).locator('.list').boundingBox();
  const side = await now(page).locator('.side').boundingBox();
  expect(side!.x).toBeGreaterThanOrEqual(list!.x + list!.width);
});
