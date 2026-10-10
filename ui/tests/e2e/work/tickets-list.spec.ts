// Tickets list on the IPC mock (TICKETS.md T2/T3): the split view following the selection, the one
// status picker from a row's status chip, and a multi-select move.
import { expect, test, type Page } from '@playwright/test';

import { boot, callsOf, dispatch } from '../shell/helpers';

const row = (page: Page, key: string) => page.locator(`[data-key="jira-acme:${key}"]`);
const chip = (page: Page, key: string) => row(page, key).locator('[data-status]');

/** Flow order on the mock: SHOP-142, SHOP-155 (Doing), SHOP-160, SHOP-120 (Waiting), SHOP-151 (Ready). */
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
  await page.keyboard.press('j');
  await expect(detail.getByRole('heading', { level: 1 })).toHaveText('Flaky test in cart service');
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

test('the row status chip opens the status picker; typing filters, Enter moves', async ({ page }) => {
  await openTickets(page, 'SHOP-151');
  await chip(page, 'SHOP-151').getByRole('button').click();
  const menu = page.getByRole('menu', { name: 'Move SHOP-151' });
  await expect(menu.getByRole('menuitem')).toHaveCount(4);
  await page.keyboard.type('prog');
  await expect(menu.getByRole('menuitem')).toHaveCount(1);
  await page.keyboard.press('Enter');
  await expect(menu).toHaveCount(0);
  await expect(chip(page, 'SHOP-151')).toContainText('In progress');
  expect((await callsOf(page, 'tracker_transition')).at(-1)?.args).toMatchObject({
    transition_id: 'to-in_progress',
  });
});

test('x, Shift+J and m move both tickets through the moves they share', async ({ page }) => {
  await openTickets(page);
  await page.keyboard.press('x');
  await page.keyboard.press('Shift+J');
  await page.keyboard.press('m');
  await expect(
    page.getByRole('menu', { name: 'Move 2 tickets' }).getByRole('menuitem').first(),
  ).toBeVisible();
  await page.keyboard.type('review');
  await page.keyboard.press('Enter');
  await expect(page.getByTestId('toasts')).toContainText('Moved 2 tickets to In review');
  await expect(chip(page, 'SHOP-142')).toContainText('In review');
  await expect(chip(page, 'SHOP-155')).toContainText('In review');
  await expect(page.locator('.k-row.picked')).toHaveCount(0);
});
