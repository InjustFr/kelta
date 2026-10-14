// Focus mode (#421, #442): ⇧⌘F / Ctrl+Shift+F hides the rail, the scope and + New, Esc Esc or Leave focus
// brings back the exact layout, and the background toasts held meanwhile (counted by a pill) come back as
// one summary.
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
  const bar = page.getByTestId('contextbar');
  await expect(bar.getByTestId('context-search')).toBeVisible();
  await expect(bar.getByTestId('list-scope')).toHaveCount(0);
  await expect(bar.getByTestId('tab-plus')).toHaveCount(0);
  await expect(page.getByTestId('work-leave-focus')).toBeVisible();
  await expect(page.getByTestId('focus-held')).toHaveCount(0);
  await page.evaluate(() =>
    window.__keltaMock!.emit({
      type: 'toast',
      toast: { level: 'info', text: 'SHOP-1 ready to review', action: null },
    }),
  );
  await expect(page.getByTestId('toasts')).not.toContainText('SHOP-1 ready to review');
  await expect(page.getByTestId('focus-held')).toHaveText('1 update held until you leave focus');

  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await expect(rail).toBeVisible();
  expect(Math.round((await rail.boundingBox())!.width)).toBe(64);
  await expect(page.getByTestId('toasts')).toContainText('While you were focused: SHOP-1 ready to review');
  await expect(page.getByTestId('focus-held')).toHaveCount(0);
  await expect(page.getByTestId('work-leave-focus')).toHaveCount(0);
  await expect(bar.getByTestId('list-scope')).toBeVisible();
  await expect(bar.getByTestId('tab-plus')).toBeVisible();
});

test('Leave focus in the work header leaves it', async ({ page }) => {
  await boot(page);
  await dispatch(page, 'view.focus');
  await page.getByTestId('work-leave-focus').click();
  await expect(page.getByTestId('rail')).toBeVisible();
  await expect(page.getByTestId('work-leave-focus')).toHaveCount(0);
});
