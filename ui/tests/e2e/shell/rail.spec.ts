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
            .locator('[data-testid="rail-settings"] .name')
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
    expect(await railWidth(page)).toBe(272);

    const edge = page.getByTestId('rail-edge');
    const box = (await edge.boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + 300);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 + 100, box.y + 300, { steps: 5 });
    await page.mouse.up();
    // The edge follows the pointer (to the pixel).
    await expect.poll(async () => Math.abs((await railWidth(page)) - 372)).toBeLessThanOrEqual(1);
    const dragged = await railWidth(page);

    await edge.focus();
    await page.keyboard.press('ArrowLeft');
    await page.keyboard.press('Shift+ArrowLeft');
    await expect.poll(() => railWidth(page)).toBe(dragged - 16 - 64);
    await expect(edge).toHaveAttribute('aria-valuenow', String(dragged - 80));
    // A click on the edge without moving keeps the keyboard-set width (no stale drag position).
    const edgeBox = (await edge.boundingBox())!;
    await page.mouse.click(edgeBox.x + edgeBox.width / 2, edgeBox.y + 300);
    expect(await railWidth(page)).toBe(dragged - 80);
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

    // Outside a terminal: a focused terminal keeps Ctrl+B (tmux prefix).
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await page.keyboard.press('Control+B');
    await expect.poll(() => railWidth(page)).toBe(416);
    await edge.focus();
    await page.keyboard.press('Enter');
    await expect.poll(() => railWidth(page)).toBe(64);
    await dispatch(page, 'rail.toggle');
    await expect.poll(() => railWidth(page)).toBe(416);
  });

  test('the Queue folds the sidebar to its lamps; Tickets and Reviews keep it open; ⌘B wins', async ({
    page,
  }) => {
    await boot(page);
    expect(await railWidth(page)).toBe(272);
    await dispatch(page, 'inbox.open');
    await expect.poll(() => railWidth(page)).toBe(64);
    await dispatch(page, 'tickets.open');
    await expect.poll(() => railWidth(page)).toBe(272);

    // Expanded by hand on the Queue: it stays open there.
    await dispatch(page, 'inbox.open');
    await expect.poll(() => railWidth(page)).toBe(64);
    await dispatch(page, 'rail.toggle');
    await expect.poll(() => railWidth(page)).toBe(272);

    // Collapsed by hand: Tickets keeps it collapsed.
    await dispatch(page, 'rail.toggle');
    await dispatch(page, 'tickets.open');
    await expect.poll(() => railWidth(page)).toBe(64);
  });

  test('every unfinished work item has a row, one without a tab resumes; the Queue folds to lamp tiles (#435)', async ({
    page,
  }) => {
    await boot(page);
    const shop = page.locator('[data-testid="rail-band"][data-project-id="shop"]');
    await expect(shop.getByTestId('rail-resume').filter({ hasText: 'SHOP-120' })).toHaveCount(1);
    // A PR waiting on a reviewer, its tab closed, has no row (#443): the Queue still lists it.
    const waiting = 'Cache headers for product images';
    await expect(shop.getByText(waiting)).toHaveCount(0);
    const coupons = shop
      .locator('[data-testid="rail-resume"], [data-testid="rail-row"]')
      .filter({ hasText: 'SHOP-155' });
    await expect(coupons).toHaveAttribute('data-testid', 'rail-resume');
    await coupons.click();
    await expect(coupons).toHaveAttribute('data-testid', 'rail-row');
    await expect(coupons).toHaveAttribute('aria-current', 'page');
    await expect(coupons.locator('.name')).toHaveText('Flaky test in cart service');
    // Home has no band and nothing sits above the bands.
    await expect(page.locator('[data-testid="rail-band"][data-project-id="home"]')).toHaveCount(0);
    await expect(page.getByTestId('rail-add')).toHaveCount(0);

    await dispatch(page, 'inbox.open');
    await expect(page.getByTestId('inbox-pane')).toContainText(waiting);
    await expect.poll(() => railWidth(page)).toBe(64);
    for (const more of await page.getByTestId('rail-more').all()) await expect(more).toBeHidden();
    await expect(coupons).toBeVisible();
  });

  test('mockup metrics (#451): 44px views, 22px captions, 30px rows, no rules; ⌘ held shows the row digits', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await boot(page);
    const height = async (sel: string) => Math.round((await page.locator(sel).first().boundingBox())!.height);
    expect(await height('[data-testid="rail-inbox"]')).toBe(44);
    expect(await height('[data-testid="rail-project"]')).toBe(22);
    expect(
      await height(
        '[data-testid="rail-band"] [data-testid="rail-resume"], [data-testid="rail-band"] [data-testid="rail-row"]',
      ),
    ).toBe(30);
    await expect(page.getByTestId('rail-inbox').locator('.kbd')).toBeVisible();
    await expect(page.getByTestId('rail-settings').locator('.kbd')).toBeVisible();
    // No rule anywhere in the expanded sidebar (keycaps and buttons keep their outlines).
    const ruled = await page.getByTestId('rail').evaluate((nav) =>
      [nav, ...nav.querySelectorAll('*')]
        .filter((e) => {
          const s = getComputedStyle(e);
          const wide = e.getBoundingClientRect().width > nav.getBoundingClientRect().width / 2;
          return (
            (wide && parseFloat(s.borderTopWidth) > 0) || (parseFloat(s.borderRightWidth) > 0 && e === nav)
          );
        })
        .map((e) => e.className),
    );
    expect(ruled).toEqual([]);

    const digits = page.getByTestId('rail').locator('.index');
    await expect(digits).toHaveCount(0);
    // boot() pretends Linux: the row chords are Ctrl+N.
    await page.keyboard.down('Control');
    await expect(digits.first()).toBeVisible();
    await page.keyboard.up('Control');
    await expect(digits).toHaveCount(0);
  });

  test('a narrow window starts with the icon column; the button opens it in full', async ({ page }) => {
    await page.setViewportSize({ width: 900, height: 600 });
    await boot(page);
    expect(await railWidth(page)).toBe(64);
    await page.getByTestId('rail-toggle').click();
    await expect.poll(() => railWidth(page)).toBe(272);
    await expect(page.getByTestId('rail-settings')).toContainText('Settings');
    await page.getByTestId('rail-toggle').click();
    await expect.poll(() => railWidth(page)).toBe(64);
  });
});
