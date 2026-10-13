import { expect, test } from '@playwright/test';
import { boot, callsOf, unfold } from './helpers';

test('the top bar has no tab list or tool strip; a work tab has no breadcrumb, its header names it', async ({
  page,
}) => {
  await boot(page);
  const bar = page.getByTestId('contextbar');
  await expect(bar.getByTestId('context-crumb')).toHaveCount(0);
  await expect(page.getByTestId('work-header-key')).toHaveText('SHOP-142');
  await expect(bar.locator('[role="tab"]')).toHaveCount(0);
  await expect(page.getByTestId('tool-strip')).toHaveCount(0);
  await expect(bar.getByTestId('tab-plus')).toBeVisible();
});

test("a rail row's × shows on hover and focus and closes it after the tab confirm", async ({ page }) => {
  // The full sidebar (not the icon column) needs a window of 1400px or more.
  await page.setViewportSize({ width: 1440, height: 900 });
  await boot(page);
  await unfold(page, 'shop');
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

test('⌥⌘1 opens the first tool zoomed in the current tab, and again hides it (#412)', async ({ page }) => {
  await boot(page, 'macos');
  const activeTab = () => page.evaluate(() => window.__kelta!.stores.layout.get('shop')!.active_tab);
  const tab = await activeTab();
  const panes = page.locator('[data-testid="pane"]:not(.hidden)');
  const before = await panes.count();
  await page.keyboard.press('Meta+Alt+Digit1');
  await expect(page.locator('[data-testid="pane"][data-zoomed="true"][data-focused="true"]')).toHaveCount(1);
  expect(await activeTab()).toBe(tab);
  expect((await callsOf(page, 'tool_open'))[0]?.args).toMatchObject({
    project_id: 'shop',
    tool_id: 'lazydocker',
    placement: 'split_right',
  });
  await expect(page.getByTestId('rail-tools')).toHaveCount(0);
  await expect(page.getByTestId('context-return')).toHaveCount(0);
  await page.keyboard.press('Meta+Alt+Digit1');
  await expect(page.locator('[data-testid="pane"][data-zoomed="true"]')).toHaveCount(0);
  await expect(panes).toHaveCount(before);
  expect(await activeTab()).toBe(tab);
});
