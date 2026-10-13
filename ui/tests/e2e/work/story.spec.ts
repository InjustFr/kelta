// The Story panel (#423): docked on the right of a work tab, open by default from 1280px, `work.story`
// toggles it per tab, its width and open state survive a reload, and Log opens on a failed start.
import { expect, test, type Page } from '@playwright/test';

import { boot } from '../shell/helpers';

const panel = (page: Page) => page.getByTestId('story-panel');
const width = async (page: Page): Promise<number> => Math.round((await panel(page).boundingBox())!.width);
const reload = async (page: Page): Promise<void> => {
  await page.reload();
  await page.waitForFunction(() => window.__kelta?.stores.projects.loaded === true);
};

test.use({ viewport: { width: 1440, height: 900 } });

test('SHOP-142: Claude open; Ctrl+I toggles; width and open state survive a reload', async ({ page }) => {
  await boot(page);
  await expect(panel(page)).toBeVisible();
  await expect(panel(page).getByTestId('story-claude')).toHaveAttribute('open', '');
  await expect(panel(page).getByTestId('story-log')).not.toHaveAttribute('open', '');
  expect(await width(page)).toBe(340);

  const edge = page.getByTestId('story-edge');
  await edge.focus();
  await page.keyboard.press('Shift+ArrowLeft');
  await expect.poll(() => width(page)).toBe(404);

  // Outside a terminal: a focused terminal on Linux keeps plain Ctrl+letter.
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.keyboard.press('Control+I');
  await expect(panel(page)).toBeHidden();
  // Another tab keeps its own state.
  await page.locator('[data-testid="rail-row"][data-tab-id="billing-t1"]').click();
  await expect(panel(page)).toBeVisible();

  await reload(page);
  await page.locator('[data-testid="rail-row"][data-tab-id="shop-t1"]').click();
  await expect(page.getByTestId('context-work')).toBeVisible();
  await expect(panel(page)).toBeHidden();
  // The phase chip opens it.
  await page.getByTestId('work-phase').click();
  await expect(panel(page)).toBeVisible();
  expect(await width(page)).toBe(404);
});

test('4567: Log opens by itself on the failed step; Skip step clears it', async ({ page }) => {
  await boot(page);
  await page.locator('[data-testid="rail-row"][data-tab-id="billing-t1"]').click();
  const log = panel(page).getByTestId('story-log');
  await expect(log).toHaveAttribute('open', '');
  await expect(log).toContainText('failed');
  await log.getByRole('button', { name: 'Skip step' }).click();
  await expect(log).not.toHaveAttribute('open', '');
});

test('a narrow window starts with the panel closed', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 700 });
  await boot(page);
  await expect(page.getByTestId('context-work')).toBeVisible();
  await expect(panel(page)).toBeHidden();
});
