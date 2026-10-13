// The mouse path for the main flows (#397): start work, review and ship with clicks only, every control
// found by what it says. The keyboard path stays covered by now.spec.ts, ship.spec.ts and the views specs.
import { expect, test, type Page } from '@playwright/test';

import { activeProject, boot, callsOf } from '../shell/helpers';

const now = (page: Page) => page.getByTestId('inbox-pane');
const detail = (page: Page) => now(page).getByTestId('now-brief');
const FLAKY = '0199a6b2-0000-7000-8000-00000000a004'; // SHOP-155, Ready for review in the mock

test.use({ viewport: { width: 1280, height: 1200 } });

test('start work from Now: the row, its Start work button, the sheet', async ({ page }) => {
  await boot(page);
  await page.getByTestId('rail-inbox').click();
  await now(page).locator('[data-row="t:jira-acme:SHOP-151"]').click();
  await detail(page)
    .getByRole('button', { name: /Start work$/ })
    .click();
  const sheet = page.getByRole('dialog', { name: /Start work on SHOP-151/ });
  await sheet.getByRole('button', { name: 'Start anyway' }).click(); // the mock has unfinished work (#354)
  await sheet.getByRole('button', { name: 'Start', exact: true }).click();
  await expect(sheet).toBeHidden();
  await expect.poll(() => activeProject(page)).toBe('shop');
  await expect(page.getByTestId('context-key')).toHaveText('SHOP-151');
});

test('start work from Tickets: the row menu lists the keys, Start work opens the sheet', async ({ page }) => {
  await boot(page);
  await page.getByTestId('rail-tickets').click();
  const pane = page.getByTestId('tickets-pane');
  const row = pane.locator('[data-key="jira-acme:SHOP-151"]');
  await row.click();
  await row.getByRole('button', { name: 'More actions' }).click();
  const menu = page.getByRole('menu', { name: 'More actions' });
  await expect(menu.getByRole('menuitem', { name: /^Assign to me/ }).locator('kbd')).toHaveText('A');
  await menu.getByRole('menuitem', { name: /^Start work/ }).click();
  await expect(page.getByRole('dialog', { name: /Start work on SHOP-151/ })).toBeVisible();
});

test('review: Ready for review opens the delta; Reviews opens a PR and approves it', async ({ page }) => {
  await boot(page);
  await page.getByTestId('rail-inbox').click();
  await now(page).locator(`[data-row="w:${FLAKY}"]`).click();
  await detail(page)
    .getByRole('button', { name: /Review changes$/ })
    .click();
  await expect(now(page)).toBeHidden();
  await expect.poll(async () => (await callsOf(page, 'work_diff')).length).toBe(1);

  await page.getByTestId('rail-reviews').click();
  const pane = page.getByTestId('reviews-pane');
  await pane.locator('.k-row').nth(1).click();
  await pane.locator('.k-row.selected').getByRole('button', { name: 'Open review (Enter)' }).click();
  const review = pane.getByTestId('review-detail');
  await expect(review).toBeFocused();
  await review.getByRole('button', { name: 'Approve' }).click();
  await expect.poll(async () => (await callsOf(page, 'review_approve')).length).toBe(1);
});

test('ship from Now: the Ship button opens the dialog, its Ship button opens the PR', async ({ page }) => {
  await boot(page);
  await page.getByTestId('rail-inbox').click();
  await now(page).locator(`[data-row="w:${FLAKY}"]`).click();
  await detail(page)
    .getByRole('button', { name: /more actions$/ })
    .click();
  await detail(page).getByRole('button', { name: 'Ship' }).click();
  const dialog = page.getByRole('dialog', { name: /^Ship SHOP-155/ });
  await expect(dialog).toBeVisible();
  await dialog.getByRole('button', { name: /^Ship/ }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByTestId('toasts').getByText(/Opened PR #\d+/)).toBeVisible();
});

test('the key list opens from the line under the list', async ({ page }) => {
  await boot(page);
  await page.getByTestId('rail-reviews').click();
  await page.getByTestId('reviews-pane').getByRole('button', { name: 'All keys' }).click();
  await expect(page.getByRole('dialog', { name: 'All keys' })).toContainText('Merge when ready');
});
