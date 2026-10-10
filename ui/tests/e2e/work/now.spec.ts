// Flow 1 on the IPC mock (FLOW §4.1): Now → Up next → start work → Claude stops with changes →
// Ready for review row → Enter opens the delta zoomed in the work tab. Also the work menu from a terminal.
import { expect, test, type Page } from '@playwright/test';

import { activeProject, boot, callsOf } from '../shell/helpers';

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
  await boot(page);
  await page.keyboard.press('Control+Shift+0');
  await expect(now(page)).toBeVisible();
  await expect(now(page).locator('[data-section="up_next"]')).toBeVisible();

  // Up next → s → start sheet → Ctrl+Enter: the work tab opens focused and Now closes.
  await select(page, 't:jira-acme:SHOP-151');
  await page.keyboard.press('s');
  const sheet = page.getByRole('dialog', { name: /Start work on SHOP-151/ });
  await expect(sheet).toBeVisible();
  await page.keyboard.press('Control+Enter');
  await expect(sheet).toBeHidden();
  await expect(now(page)).toBeHidden();
  await expect.poll(() => activeProject(page)).toBe('shop');
  const header = page.getByTestId('work-header');
  await expect(header).toContainText('SHOP-151');

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
  await expect(header).toContainText('+30');

  // Now: the item sits in Ready for review with Claude's last message; Enter opens the delta.
  await page.keyboard.press('Control+Shift+0');
  const toReview = now(page).locator('[data-section="to_review"]');
  await expect(toReview).toBeVisible();
  const id = await page.evaluate(
    () => window.__kelta!.stores.work.all.find((w) => w.ticket?.key === 'SHOP-151')!.id,
  );
  await select(page, `w:${id}`);
  await expect(now(page).getByTestId('now-detail')).toContainText('Tax lines added.');
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
  await page.keyboard.press('Control+Shift+Period');
  const menu = page.getByRole('menu', { name: 'Work' });
  await expect(menu).toBeVisible();
  await expect(menu.getByRole('menuitem')).toHaveCount(20);
  await expect(menu.getByRole('menuitem', { name: /Continue rebase/ })).toHaveAttribute(
    'title',
    'Only while a rebase is stopped',
  );
  await page.keyboard.press('Escape');
  await expect(menu).toBeHidden();
});

test('Now open refreshes review items: a moved head reads Updated since your review', async ({ page }) => {
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
  await page.keyboard.press('Control+Shift+0');
  const row = now(page).locator(`[data-row="w:${id}"]`);
  await expect(row).toContainText('Reviewed');
  // #101's head moved after my review: back in Review requests.
  await expect(now(page).locator('[data-row^="r:"][data-row$="#101"]')).toContainText(
    'Updated since your review',
  );

  // Someone pushes to #309 while Now is closed; opening Now asks the host again (`review_get`).
  await page.evaluate(() => {
    window.__kelta!.stores.ui.inboxActive = false;
    const pr = window.__keltaMock!.state.reviews.find((r) => r.review.ref.number === 309)!;
    pr.review.head_sha = '3'.repeat(40);
  });
  await expect(now(page)).toBeHidden();
  const before = (await callsOf(page, 'review_get')).length;
  await page.keyboard.press('Control+Shift+0');
  await expect(row).toContainText('Updated since your review');
  const gets = (await callsOf(page, 'review_get')).slice(before);
  expect(gets.some((c) => (c.args as { review: { number: number } }).review.number === 309)).toBe(true);
});
