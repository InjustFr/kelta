// ⌘. on any selection (#404), on the IPC mock: the selected row's menu in the Queue, the Tickets list and
// board and Reviews, its next step first and lit so ⌘. ↵ runs it; a focused detail's More menu.
import { expect, test, type Page } from '@playwright/test';

import { boot, callsOf, dispatch } from '../shell/helpers';

const menu = (page: Page, name = 'More actions') => page.getByRole('menu', { name });
const first = (page: Page, name?: string) => menu(page, name).getByRole('menuitem').first();
const firstLabel = (page: Page, name?: string) => first(page, name).locator('.label');

/** `⌘.` on Linux. */
const actionsKey = (page: Page) => page.keyboard.press('Control+.');

test('Queue: ⌘. opens the selected row’s actions, ↵ runs its next step', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 1200 });
  await boot(page);
  await page.keyboard.press('Control+0');
  const now = page.getByTestId('inbox-pane');
  const row = 'w:0199a6b2-0000-7000-8000-00000000a004'; // SHOP-155, ready for review
  for (let i = 0; i < 30; i += 1) {
    if ((await now.locator('.row[aria-current="true"]').getAttribute('data-row')) === row) break;
    await page.keyboard.press('j');
  }
  await actionsKey(page);
  await expect(firstLabel(page, 'Work')).toHaveText(/^Review changes/);
  await expect(first(page, 'Work')).toHaveClass(/active/);
  await page.keyboard.press('Enter');
  await expect(menu(page, 'Work')).toHaveCount(0);
  await expect.poll(async () => (await callsOf(page, 'work_diff')).length).toBe(1);
});

async function openTickets(page: Page, mode: 'List' | 'Board'): Promise<void> {
  await boot(page);
  await dispatch(page, 'tickets.open', { project_id: 'shop' });
  await page.getByRole('tab', { name: mode }).click();
  await page.getByRole('tab', { name: /^Team/ }).click();
}

test('Tickets list: ⌘. on the row starts work with ↵; in the detail, its More menu', async ({ page }) => {
  await openTickets(page, 'List');
  const row = page.locator('[data-key="jira-acme:SHOP-151"]');
  await row.locator('.k-row-key').click();
  await expect(row).toHaveAttribute('aria-current', 'true');
  await actionsKey(page);
  await expect(firstLabel(page)).toHaveText(/^Start work/);
  await expect(first(page)).toHaveClass(/active/);
  await page.keyboard.press('Enter');
  await expect(page.getByRole('dialog', { name: /Start work on SHOP-151/ })).toBeVisible();
  await page.keyboard.press('Escape');

  // The detail column has the keys: its More menu (Copy branch is not a row action).
  await page.getByTestId('ticket-detail').focus();
  await actionsKey(page);
  await expect(menu(page).getByRole('menuitem', { name: /^Copy branch/ })).toBeVisible();
});

test('Tickets board: ⌘. on the card opens its menu, Start work first', async ({ page }) => {
  await openTickets(page, 'Board');
  const card = page.locator('.card[data-key="jira-acme:SHOP-151"]');
  await card.click();
  await expect(card).toHaveAttribute('aria-current', 'true');
  await actionsKey(page);
  await expect(firstLabel(page)).toHaveText(/^Start work/);
  await expect(first(page)).toHaveClass(/active/);
});

test('Reviews: ⌘. opens the selected PR’s menu, ↵ opens the review', async ({ page }) => {
  await boot(page);
  await page.getByTestId('rail-reviews').click();
  const pane = page.getByTestId('reviews-pane');
  await expect(pane.locator('.row.selected')).toBeVisible();
  await pane.focus();
  await actionsKey(page);
  await expect(firstLabel(page)).toHaveText(/^Open review/);
  await expect(first(page)).toHaveClass(/active/);
  await page.keyboard.press('Enter');
  await expect(menu(page)).toHaveCount(0);
  await expect(page.getByTestId('review-detail')).toBeFocused();

  // The detail has the keys: ⌘. expands its actions (Request changes is not a row action), not the row menu.
  const brief = page.getByTestId('review-brief');
  await expect(brief).toBeVisible();
  await actionsKey(page);
  await expect(brief.getByRole('button', { name: /Request changes/ })).toBeVisible();
  await expect(menu(page)).toHaveCount(0);
});
