// The context bar (#248): one 30px bar above the panes, the window title follows its breadcrumb
// (native decorations on mock IPC), no work segment (a work tab's header sits above its panes, #428),
// and narrow windows drop items in the stated order.
import { expect, test, type Page } from '@playwright/test';

import { boot, unfold } from './helpers';

const bar = (page: Page) => page.getByTestId('contextbar');

test('one 30px bar; the window title follows the breadcrumb', async ({ page }) => {
  await boot(page);
  expect((await bar(page).boundingBox())?.height).toBe(30);
  await expect(page.getByTestId('titlebar')).toHaveCount(0);
  await expect(page).toHaveTitle(/^Shop › SHOP-142 Rate-limit login/);
  await unfold(page, 'shop');
  await page.locator('[data-testid="rail-row"][data-tab-id="shop-t2"]').click();
  await expect(page).toHaveTitle('Shop › ~/code/shop-api');
  await page.getByTestId('rail-inbox').click();
  await expect(page).toHaveTitle('Now');
  await expect(bar(page).getByTestId('context-underneath')).toContainText('Shop › Shell');
});

test('a work tab has its header above the panes, and keeps it over a zoomed pane', async ({ page }) => {
  await boot(page);
  const header = page.getByTestId('work-header');
  await expect(header).toBeVisible();
  await expect(bar(page).getByTestId('work-header')).toHaveCount(0);
  const top = (await header.boundingBox())!;
  const workPanes = (await page.getByTestId('panes').boundingBox())!;
  expect(workPanes.y).toBeGreaterThanOrEqual(top.y + top.height);
  await page.keyboard.press('Control+Shift+Enter'); // pane.zoom
  await expect(header).toBeVisible();
  await unfold(page, 'shop');
  await page.locator('[data-testid="rail-row"][data-tab-id="shop-t2"]').click();
  await expect(header).toHaveCount(0);
  const shellPanes = (await page.getByTestId('panes').boundingBox())!;
  expect(shellPanes.y).toBeLessThan(workPanes.y);
});

test('narrow windows drop items in order; ‹, the key and + stay', async ({ page }) => {
  await boot(page);
  const at = async (width: number) => page.setViewportSize({ width, height: 800 });
  const title = bar(page).locator('.ttl');

  await at(1400);
  await expect(title).toBeVisible();
  await at(900);
  await expect(title).toBeVisible();
  await at(820);
  await expect(title).toBeHidden();
  await at(640);
  for (const id of ['context-back', 'context-key', 'tab-plus', 'work-menu-button'])
    await expect(page.getByTestId(id)).toBeInViewport({ ratio: 1 });
});
