// One project scope for the global lists (#409): the top bar's menu sets it (#429), Tickets, Reviews and
// Now follow it, keep it when switching view, and it survives a reload.
import { expect, test, type Page } from '@playwright/test';

import { boot } from './helpers';

const host = (page: Page) => page.getByTestId('global-host');
const scopeOf = (page: Page) => page.getByTestId('contextbar').getByTestId('list-scope').locator('select');

test('Tickets: scoping to Shop leaves its tickets, and Reviews opens in that scope', async ({ page }) => {
  await boot(page);
  await page.getByTestId('rail-tickets').click();
  const shop = host(page).locator('[data-key="jira-acme:SHOP-142"]');
  const billing = host(page).locator('[data-key="redmine-corp:4567"]');
  await expect(shop).toBeVisible();
  await expect(billing).toBeVisible();

  await scopeOf(page).selectOption('shop');
  await expect(billing).toHaveCount(0);
  await expect(shop).toBeVisible();

  await page.getByTestId('rail-reviews').click();
  await expect(scopeOf(page)).toHaveValue('shop');
  await page.reload();
  await expect(scopeOf(page)).toHaveValue('shop');
});

test('Reviews: scoping to Billing leaves its pull requests, All projects brings the rest back', async ({
  page,
}) => {
  await boot(page);
  await page.getByTestId('rail-reviews').click();
  const pane = host(page).getByTestId('reviews-pane');
  const shopPr = pane.locator('[data-key="github-acme:acme/shop-api#311"]');
  const billingPr = pane.locator('[data-key="gitlab-corp:fin/billing/core#42"]');
  await expect(shopPr).toBeVisible();
  await expect(billingPr).toBeVisible();

  await scopeOf(page).selectOption('billing');
  await expect(shopPr).toHaveCount(0);
  await expect(billingPr).toBeVisible();

  await scopeOf(page).selectOption('');
  await expect(shopPr).toBeVisible();
});

test('Now: scoping to Billing hides Shop rows and the header counts follow', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 1200 });
  await boot(page);
  await page.keyboard.press('Control+Shift+Digit0');
  const now = host(page).getByTestId('inbox-pane');
  const shopTicket = now.locator('[data-row="t:jira-acme:SHOP-151"]');
  await expect(shopTicket).toBeVisible();
  const header = now.getByTestId('now-header');
  const before = await header.textContent();

  await scopeOf(page).selectOption('billing');
  await expect(shopTicket).toHaveCount(0);
  await expect(header).not.toHaveText(before ?? '');

  await page.getByTestId('rail-tickets').click();
  await expect(scopeOf(page)).toHaveValue('billing');
});
