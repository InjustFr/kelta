import { expect, test } from '@playwright/test';
import { boot } from './helpers';

test('the context bar names the project and the row, with no tab list or tool strip', async ({ page }) => {
  await boot(page);
  const bar = page.getByTestId('contextbar');
  await expect(bar.getByTestId('context-crumb')).toHaveText(/Shop\s*›\s*SHOP-142\s*Rate-limit login/);
  await expect(bar.locator('[role="tab"]')).toHaveCount(0);
  await expect(page.getByTestId('tool-strip')).toHaveCount(0);
  await expect(bar.getByTestId('tab-plus')).toBeVisible();
});

test("a rail row's × shows on hover and focus and closes it after the tab confirm", async ({ page }) => {
  // The full sidebar (not the icon column) needs a window of 1400px or more.
  await page.setViewportSize({ width: 1440, height: 900 });
  await boot(page);
  const rows = page.locator('[data-testid="rail-row"][data-project-id="shop"]');
  const row = page.locator('li', { has: page.locator('[data-tab-id="shop-t2"]') });
  const close = row.getByRole('button', { name: 'Close Shell' });
  await expect(close).toBeHidden();
  await row.hover();
  await expect(close).toBeVisible();
  await page.mouse.move(0, 0);
  await row.getByTestId('rail-row').focus();
  await expect(close).toBeVisible();
  await row.hover();
  await close.click();
  // The Shell tab runs sessions, so close asks first.
  await page.getByRole('button', { name: 'Close tab, keep sessions' }).click();
  await expect(rows).toHaveCount(2);
});

test('⌥⌘1 opens the first tool from Now, and again returns to Now', async ({ page }) => {
  await boot(page, 'macos');
  await page.getByTestId('rail-inbox').click();
  await expect(page.getByTestId('global-host')).toBeVisible();
  await page.keyboard.press('Meta+Alt+Digit1');
  await expect(page.getByTestId('global-host')).toBeHidden();
  await expect(page.locator('[data-testid="rail-tool-instance"][data-tool-id="lazydocker"]')).toHaveCount(2);
  await page.keyboard.press('Meta+Alt+Digit1');
  await expect(page.getByTestId('global-host')).toHaveAttribute('data-view', 'now');
});
