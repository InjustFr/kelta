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

test('the tool strip stays in view when tabs overflow and opens tools from the keyboard', async ({
  page,
}) => {
  await boot(page);
  const strip = page.getByTestId('tool-strip');
  await expect(strip.getByRole('button')).toHaveCount(4);

  // Narrow enough for the tabs to overflow: the strip stays pinned to the right edge at both ends.
  await page.setViewportSize({ width: 640, height: 600 });
  const bar = page.getByTestId('tabbar');
  await expect.poll(() => bar.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(true);
  for (const at of ['start', 'end'] as const) {
    await bar.evaluate((el, a) => (el.scrollLeft = a === 'start' ? 0 : el.scrollWidth), at);
    const [b, s] = [await bar.boundingBox(), await strip.boundingBox()];
    expect(Math.abs(b!.x + b!.width - (s!.x + s!.width))).toBeLessThan(16);
  }

  // Tab order: New tab, then the tools.
  await page.getByTestId('tab-plus').focus();
  await page.keyboard.press('Tab');
  await expect(strip.getByRole('button').first()).toBeFocused();
  const lazygit = strip.getByRole('button', { name: 'lazygit' });
  await expect(lazygit).toHaveAttribute('aria-pressed', 'false');
  await page.keyboard.press('Tab');
  await expect(lazygit).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(lazygit).toHaveAttribute('aria-pressed', 'true');
  const htop = strip.getByRole('button', { name: 'htop' });
  await htop.focus();
  await page.keyboard.press('Space');
  await expect(htop).toHaveAttribute('aria-pressed', 'true');
});
