// Tickets list on the IPC mock (TICKETS.md T2/T3): the split view following the selection, the status
// picker in the reading pane, and a multi-select move.
import { expect, test, type Page } from '@playwright/test';

import { boot, callsOf, dispatch } from '../shell/helpers';

const row = (page: Page, key: string) => page.locator(`[data-key="jira-acme:${key}"]`);
const group = (page: Page, status: string) => page.locator(`[data-group="${status}"] .count`);

/** Status order on the mock (#400): To Do (SHOP-151, SHOP-155), In Progress (SHOP-142), In Review (SHOP-120), Blocked (SHOP-160). */
async function openTickets(page: Page, select = 'SHOP-142'): Promise<void> {
  await boot(page);
  await dispatch(page, 'tickets.open', { project_id: 'shop' });
  await page.getByRole('tab', { name: 'List' }).click(); // the mock layout opens the board
  await row(page, select).locator('.k-row-key').click();
  await expect(row(page, select)).toHaveAttribute('aria-current', 'true');
}

test('split view follows the selection; Space toggles, Enter focuses, Esc returns', async ({ page }) => {
  await openTickets(page);
  const detail = page.getByTestId('ticket-detail');
  await expect(detail.getByRole('heading', { level: 1 })).toHaveText('Rate-limit login attempts');
  await page.keyboard.press('j'); // the In Review header
  await page.keyboard.press('j');
  await expect(detail.getByRole('heading', { level: 1 })).toHaveText('Upgrade payment SDK');
  await page.keyboard.press('Enter');
  await expect(detail).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('tickets-pane')).toBeFocused();
  await page.keyboard.press('Space');
  await expect(detail).toHaveCount(0);
  await page.keyboard.press('Shift+Enter');
  await expect(page.getByTestId('ticket-detail')).toHaveCount(1); // the standalone pane
  await expect(page.getByTestId('tickets-pane')).toBeVisible();
});

test('the status picker in the reading pane: typing filters, Enter moves', async ({ page }) => {
  await openTickets(page, 'SHOP-151');
  const status = page.getByTestId('ticket-detail').getByRole('button', { name: /^Status To Do/ });
  await status.click();
  const menu = page.getByRole('menu', { name: 'Move SHOP-151' });
  await expect(menu.getByRole('menuitem')).toHaveCount(4);
  await page.keyboard.type('prog');
  await expect(menu.getByRole('menuitem')).toHaveCount(1);
  await page.keyboard.press('Enter');
  await expect(menu).toHaveCount(0);
  await expect(
    page.getByTestId('ticket-detail').getByRole('button', { name: /^Status In progress/ }),
  ).toBeVisible();
  expect((await callsOf(page, 'tracker_transition')).at(-1)?.args).toMatchObject({
    transition_id: 'to-in_progress',
  });
});

test('x, Shift+J and m move both tickets through the moves they share', async ({ page }) => {
  await openTickets(page, 'SHOP-151');
  await page.keyboard.press('x');
  await page.keyboard.press('Shift+J');
  await page.keyboard.press('m');
  await expect(
    page.getByRole('menu', { name: 'Move 2 tickets' }).getByRole('menuitem').first(),
  ).toBeVisible();
  await page.keyboard.type('review');
  await page.keyboard.press('Enter');
  await expect(page.getByTestId('toasts')).toContainText('Moved 2 tickets to In review');
  await expect(group(page, 'In review')).toHaveText('2');
  await expect(page.locator('.k-row.picked')).toHaveCount(0);
});

test('a background run claim shows on the ticket row and follows report_progress', async ({ page }) => {
  await openTickets(page, 'SHOP-160');
  const report = (state: string, phase: string, note: string | null) =>
    page.evaluate(
      ([state, phase, note]) => {
        const mock = window.__keltaMock!;
        const ref = mock.state.work[0]!.ticket!;
        const w = {
          ...structuredClone(mock.state.work[0]!),
          id: 'wi-run-160',
          ticket: { ...ref, key: 'SHOP-160' },
          session_ids: [],
          tab_id: null,
          run: {
            name: 'kelta-ticket-pass',
            run_id: 'r1',
            session_id: 'orch',
            phase,
            state: state as 'running',
            note,
            sha: null,
          },
        };
        mock.emit({ type: 'work.updated', work: w });
      },
      [state, phase, note] as const,
    );
  // The row's stage track (#400): with Claude, waiting while it runs, broken once it is blocked.
  const mark = row(page, 'SHOP-160').locator('[data-mark]');
  const run = page.getByTestId('ticket-detail').locator('[data-run]');

  await report('running', 'Implement', null);
  await expect(mark).toHaveAttribute('data-mark', 'wait');
  await expect(run).toContainText('kelta-ticket-pass · Implement');

  await report('blocked', 'Review', '2 unresolved review findings');
  await expect(mark).toHaveAttribute('data-mark', 'broken');
  await expect(run).toContainText('kelta-ticket-pass · Review');
  await expect(run).toHaveAttribute('title', '2 unresolved review findings');

  // Now lists the blocked run under Needs you, with its note.
  await page.keyboard.press('Control+0');
  const nowRow = page.getByTestId('inbox-pane').locator('[data-row="w:wi-run-160"]');
  await expect(nowRow).toContainText('2 unresolved review findings');
  await expect(nowRow.locator('[data-mark]')).toHaveAttribute('data-mark', 'broken');
});

test('b in the detail blocks the ticket: the row shows Blocked; b again unblocks', async ({ page }) => {
  await openTickets(page);
  await page.keyboard.press('Enter');
  await page.keyboard.press('b');
  await expect(row(page, 'SHOP-142').locator('[data-blocked]')).toHaveText('Blocked');
  await page.keyboard.press('b');
  await expect(row(page, 'SHOP-142').locator('[data-blocked]')).toHaveCount(0);
  expect((await callsOf(page, 'tracker_set_blocked')).map((c) => c.args)).toMatchObject([
    { blocked: true },
    { blocked: false },
  ]);
});

test("a source's chosen tracker fields show as chips on the row and in the detail; no value, no chip", async ({
  page,
}) => {
  await openTickets(page);
  const fields = row(page, 'SHOP-142').locator('[data-field]');
  await expect(fields).toHaveText(['Verified: Yes', 'EU']);
  await expect(row(page, 'SHOP-155').locator('[data-field]')).toHaveCount(0);
  const detail = page.getByTestId('ticket-detail');
  await expect(detail.locator('dd[data-field]')).toHaveText(['Yes', 'EU']);
  // the chips stay inside the title cell (they give way with the title, never past it)
  for (const width of [1440, 900]) {
    await page.setViewportSize({ width, height: 900 });
    const out = await row(page, 'SHOP-142')
      .locator('.c-what')
      .evaluate((cell) => {
        const right = cell.getBoundingClientRect().right;
        return [...cell.querySelectorAll<HTMLElement>('[data-field]')].filter(
          (f) => f.checkVisibility() && f.getBoundingClientRect().right > right + 0.5,
        ).length;
      });
    expect(out, `chips past the title at ${width}px`).toBe(0);
  }
});
