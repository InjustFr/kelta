// Focus mode (#421): ⇧⌘F / Ctrl+Shift+F hides the rail, Esc Esc brings back the exact layout, and the
// background toasts held meanwhile come back as one summary.
import { expect, test } from '@playwright/test';

import { boot, dispatch } from './helpers';

test.use({ viewport: { width: 1920, height: 1080 } });

test('hides the rail, holds background toasts and restores the layout with one summary', async ({ page }) => {
  await boot(page);
  const rail = page.getByTestId('rail');
  await dispatch(page, 'rail.toggle'); // collapsed by hand: focus mode must give it back collapsed
  await expect.poll(async () => Math.round((await rail.boundingBox())!.width)).toBe(64);

  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.keyboard.press('Control+Shift+F');
  await expect(rail).toHaveCount(0);
  await page.evaluate(() =>
    window.__keltaMock!.emit({
      type: 'toast',
      toast: { level: 'info', text: 'SHOP-1 ready to review', action: null },
    }),
  );
  await expect(page.getByTestId('toasts')).not.toContainText('SHOP-1 ready to review');

  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await expect(rail).toBeVisible();
  expect(Math.round((await rail.boundingBox())!.width)).toBe(64);
  await expect(page.getByTestId('toasts')).toContainText('While you were focused: SHOP-1 ready to review');
});
