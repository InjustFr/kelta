// The context bar (#248): one 30px bar above the panes, the window title follows its breadcrumb
// (native decorations on mock IPC), a work tab has no header row of its own, and narrow windows drop
// items in the stated order.
import { expect, test, type Page } from '@playwright/test';

import { boot } from './helpers';

const bar = (page: Page) => page.getByTestId('contextbar');

test('one 30px bar; the window title follows the breadcrumb', async ({ page }) => {
  await boot(page);
  expect((await bar(page).boundingBox())?.height).toBe(30);
  await expect(page.getByTestId('titlebar')).toHaveCount(0);
  await expect(page).toHaveTitle(/^Shop › SHOP-142 Rate-limit login/);
  await page.locator('[data-testid="rail-row"][data-tab-id="shop-t2"]').click();
  await expect(page).toHaveTitle('Shop › ~/code/shop-api');
  await page.getByTestId('rail-inbox').click();
  await expect(page).toHaveTitle('Now');
  await expect(bar(page).getByTestId('context-underneath')).toContainText('Shop › Shell');
});

test('a work tab has no header row: its panes start right under the bar, like any tab', async ({ page }) => {
  await boot(page);
  await expect(page.getByTestId('context-work')).toBeVisible();
  const workPanes = await page.getByTestId('panes').boundingBox();
  await page.locator('[data-testid="rail-row"][data-tab-id="shop-t2"]').click();
  await expect(page.getByTestId('context-work')).toHaveCount(0);
  const shellPanes = await page.getByTestId('panes').boundingBox();
  expect(workPanes?.y).toBe(shellPanes?.y);
  expect(workPanes?.height).toBe(shellPanes?.height);
});

test('narrow windows drop items in order; ‹, the key and + stay', async ({ page }) => {
  await boot(page);
  const at = async (width: number) => page.setViewportSize({ width, height: 800 });
  const work = page.getByTestId('context-work');
  const branch = page.getByTestId('work-branch');
  const ahead = page.getByTestId('ahead');
  const ticket = page.getByTestId('work-status');
  const title = bar(page).locator('.ttl');
  const phaseLabel = page.getByTestId('work-phase').locator('.phase-label');
  const workLabel = page.getByTestId('work-menu-button').locator('.label');

  await at(1400);
  await expect(work).toBeVisible();
  for (const el of [branch, ahead, ticket, title, phaseLabel, workLabel]) await expect(el).toBeVisible();
  await at(1200);
  await expect(branch).toBeHidden();
  await expect(ahead).toBeVisible();
  await at(1050);
  await expect(ahead).toBeHidden();
  await expect(ticket).toBeVisible();
  await at(900);
  await expect(ticket).toBeHidden();
  await expect(title).toBeVisible();
  await at(820);
  await expect(title).toBeHidden();
  await expect(phaseLabel).toBeVisible();
  await at(740);
  await expect(phaseLabel).toBeHidden();
  await expect(workLabel).toBeVisible();
  await at(640);
  await expect(workLabel).toBeHidden();
  for (const id of ['context-back', 'context-key', 'work-phase', 'work-menu-button', 'tab-plus'])
    await expect(page.getByTestId(id)).toBeInViewport({ ratio: 1 });
});
