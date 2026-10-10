import { expect, test } from '@playwright/test';
import { boot } from './helpers';

test('an inactive tab keeps its width on hover and focus, and its cross closes it', async ({ page }) => {
  await boot(page);
  const tab = page.locator('.tabbar [role="tab"]:not(.active)').first();
  const close = tab.locator('.close');
  const width = () => tab.evaluate((el) => el.getBoundingClientRect().width);

  const rest = await width();
  await expect(close).toBeHidden();

  await tab.hover();
  await expect(close).toBeVisible();
  expect(await width()).toBe(rest);

  await page.mouse.move(0, 0);
  await tab.focus();
  await expect(close).toBeVisible();
  expect(await width()).toBe(rest);

  const before = await page.locator('.tabbar [role="tab"]').count();
  await tab.hover();
  await close.click();
  // The mock tab runs a session, so close asks first.
  await page.getByRole('button', { name: 'Close tab, keep sessions' }).click();
  await expect(page.locator('.tabbar [role="tab"]')).toHaveCount(before - 1);
});
