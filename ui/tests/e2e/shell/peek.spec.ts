// Peek & reply (ticket #139): the popover opens from a rail row, a Now row and the key,
// answers through session_write and never attaches a terminal to the peeked session.
import { expect, test, type Page } from '@playwright/test';

import { boot, callsOf, SESSIONS } from './helpers';

const peek = (page: Page) => page.getByTestId('peek');
const textarea = (page: Page) =>
  page.locator('[data-testid="pane"][data-focused="true"] .xterm-helper-textarea');

async function attached(page: Page, id: string): Promise<boolean> {
  return (await callsOf(page, 'session_attach')).some((c) => c.args?.id === id);
}

test.describe('peek', () => {
  test('rail row hover: tail without attaching, Esc gives focus back to the pane', async ({ page }) => {
    await boot(page);
    await textarea(page).focus();
    await page
      .locator(`[data-testid="rail-row"][data-tab-id="billing-t1"]`)
      .hover({ position: { x: 20, y: 10 } });
    await expect(peek(page)).toHaveAttribute('data-session-id', SESSIONS.billingClaude);
    await expect(page.getByTestId('peek-tail')).not.toBeEmpty();
    expect(await attached(page, SESSIONS.billingClaude)).toBe(false);

    await peek(page).hover();
    await expect(page.getByTestId('peek-reply')).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(peek(page)).toHaveCount(0);
    await expect(textarea(page)).toBeFocused();
  });

  test('the key opens it from a terminal; a reply goes as a bracketed paste then Enter', async ({ page }) => {
    await boot(page);
    await textarea(page).focus();
    await page.keyboard.press('Control+Shift+Y');
    await expect(peek(page)).toHaveAttribute('data-session-id', SESSIONS.billingClaude);
    await page.keyboard.type('yes');
    await page.keyboard.press('Enter');
    await expect(peek(page)).toHaveCount(0);
    await expect
      .poll(async () =>
        (await callsOf(page, 'session_write'))
          .filter((c) => c.args?.id === SESSIONS.billingClaude)
          .map((c) => String.fromCharCode(...Object.values(c.args?.data as Record<string, number>))),
      )
      .toEqual(['\x1b[200~yes\x1b[201~', '\r']);
    expect(await attached(page, SESSIONS.billingClaude)).toBe(false);
  });

  test('another row and a Now row peek their own session', async ({ page }) => {
    await boot(page);
    await page
      .locator('[data-testid="rail-row"][data-tab-id="shop-t1"]')
      .hover({ position: { x: 20, y: 10 } });
    await expect(peek(page)).toHaveAttribute('data-session-id', SESSIONS.shopClaude);
    await page.mouse.move(0, 0);
    await page.mouse.click(1, 400);
    await expect(peek(page)).toHaveCount(0);

    await page.getByTestId('rail-inbox').click();
    await // Billing Claude belongs to a work item: its Now row peeks at that Claude.
    await page.locator(`[data-row="w:0199a6b2-0000-7000-8000-00000000a002"]`).hover();
    await expect(peek(page)).toHaveAttribute('data-session-id', SESSIONS.billingClaude);
  });

  test('the reply is one control-height line; a long last message wraps at 480px', async ({ page }) => {
    await boot(page);
    const long = `Done.\n\n${'Refactored the payment retry wrapper and its tests. '.repeat(8)}\n${'x'.repeat(300)}`;
    await page.evaluate(
      ([id, preview]) => {
        const { sessions } = window.__kelta!.stores;
        const s = sessions.get(id)!;
        sessions.upsert({ ...s, status: 'done', attention: 'done', claude: { ...s.claude!, preview } });
      },
      [SESSIONS.shopClaude, long] as const,
    );
    await page
      .locator('[data-testid="rail-row"][data-tab-id="shop-t1"]')
      .hover({ position: { x: 20, y: 10 } });
    await expect(page.getByTestId('peek-message')).toContainText('Refactored the payment');

    const control = await page.evaluate(() =>
      parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--k-control-height-sm')),
    );
    const reply = (await page.getByTestId('peek-reply').boundingBox())!;
    expect(reply.height).toBeLessThanOrEqual(control + 2);
    const box = (await peek(page).boundingBox())!;
    expect(reply.width).toBeGreaterThan(box.width - 40); // full width, not the toolbar's 240px

    const overflows = await page.evaluate(() =>
      ['peek', 'peek-message'].map((t) => {
        const el = document.querySelector<HTMLElement>(`[data-testid="${t}"]`)!;
        return el.scrollWidth > el.clientWidth;
      }),
    );
    expect(overflows).toEqual([false, false]);
  });
});
