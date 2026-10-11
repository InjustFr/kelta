// Layout at 1920×1080 and at the minimum window size (#362): Settings and the rail are never cut off,
// and the rail resizes and collapses by mouse and keyboard, both kept across a restart.
import { expect, test, type Page } from '@playwright/test';

import { boot, dispatch } from './helpers';

const railWidth = async (page: Page): Promise<number> =>
  Math.round((await page.getByTestId('rail').boundingBox())!.width);

for (const locale of ['en-US', 'fr-FR', 'de-DE']) {
  for (const [width, height] of [
    [1920, 1080],
    [640, 400],
  ] as const) {
    test.describe(`${locale} at ${width}×${height}`, () => {
      test.use({ locale, viewport: { width, height } });

      test('every Settings section is reachable and no rail or section label is cut off', async ({
        page,
      }) => {
        await boot(page);
        if (width === 1920) {
          // The labels beside a chord badge are whole: the badge gives way, not the label.
          const cut = await page
            .locator('[data-testid="rail-search"] .search-text, [data-testid="rail-settings"] .name')
            .evaluateAll((els) => els.filter((e) => e.scrollWidth > e.clientWidth).map((e) => e.textContent));
          expect(cut).toEqual([]);
        }

        await dispatch(page, 'settings.open', { section: 'general' });
        const nav = page.locator('[data-testid="settings-pane"] nav');
        const buttons = nav.locator('button[data-section]');
        await expect(buttons.first()).toBeVisible();
        // No row is squeezed under its own label (they used to shrink and overlap).
        const squeezed = await buttons.evaluateAll((bs) =>
          bs
            .filter((b) => b.scrollHeight > b.clientHeight + 1 || b.clientHeight < 24)
            .map((b) => b.textContent),
        );
        expect(squeezed).toEqual([]);
        const last = buttons.last();
        await last.scrollIntoViewIfNeeded();
        await last.click();
        await expect(last).toHaveAttribute('aria-current', 'page');
        await expect(page.locator('[data-testid="settings-pane"] main h1').first()).toBeInViewport();
      });
    });
  }
}

test.describe('sidebar', () => {
  test.use({ viewport: { width: 1920, height: 1080 } });

  test('resizes by drag and arrow keys, collapses by button, shortcut and Enter, and keeps both after a reload', async ({
    page,
  }) => {
    await boot(page);
    const toggle = page.getByTestId('rail-toggle');
    expect(await railWidth(page)).toBe(288);

    const edge = page.getByTestId('rail-edge');
    const box = (await edge.boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + 300);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 + 100, box.y + 300, { steps: 5 });
    await page.mouse.up();
    // The edge follows the pointer (to the pixel).
    await expect.poll(async () => Math.abs((await railWidth(page)) - 388)).toBeLessThanOrEqual(1);
    const dragged = await railWidth(page);

    await edge.focus();
    await page.keyboard.press('ArrowLeft');
    await page.keyboard.press('Shift+ArrowLeft');
    await expect.poll(() => railWidth(page)).toBe(dragged - 16 - 64);
    await expect(edge).toHaveAttribute('aria-valuenow', String(dragged - 80));
    // Clamped to its bounds.
    for (let i = 0; i < 10; i += 1) await page.keyboard.press('Shift+ArrowRight');
    await expect.poll(() => railWidth(page)).toBe(480);
    await page.keyboard.press('Shift+ArrowLeft');

    await page.reload();
    await page.waitForFunction(() => window.__kelta?.stores.projects.loaded === true);
    expect(await railWidth(page)).toBe(416);

    await toggle.click();
    await expect.poll(() => railWidth(page)).toBe(64);
    await expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await expect(edge).toHaveCount(0);

    await page.reload();
    await page.waitForFunction(() => window.__kelta?.stores.projects.loaded === true);
    expect(await railWidth(page)).toBe(64);

    // Mod+B is Ctrl+Shift+S on Linux.
    await page.keyboard.press('Control+Shift+S');
    await expect.poll(() => railWidth(page)).toBe(416);
    await edge.focus();
    await page.keyboard.press('Enter');
    await expect.poll(() => railWidth(page)).toBe(64);
    await dispatch(page, 'rail.toggle');
    await expect.poll(() => railWidth(page)).toBe(416);
  });

  test('a narrow window starts with the icon column; the button opens it in full', async ({ page }) => {
    await page.setViewportSize({ width: 900, height: 600 });
    await boot(page);
    expect(await railWidth(page)).toBe(64);
    await page.getByTestId('rail-toggle').click();
    await expect.poll(() => railWidth(page)).toBe(288);
    await expect(page.getByTestId('rail-search')).toContainText('Search or run…');
    await page.getByTestId('rail-toggle').click();
    await expect.poll(() => railWidth(page)).toBe(64);
  });
});
