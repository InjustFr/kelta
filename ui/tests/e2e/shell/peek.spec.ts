// Peek & reply (ticket #139): the popover opens from a rail lamp, a tab dot, a Now row and the key,
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
  test('rail lamp hover: tail without attaching, Esc gives focus back to the pane', async ({ page }) => {
    await boot(page);
    await textarea(page).focus();
    await page
      .locator(`[data-testid="rail-project"][data-project-id="billing"]`)
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

  test('tab dot and Now row hover open it too', async ({ page }) => {
    await boot(page);
    await page.locator('[data-testid="tab"] .lamp-slot').first().hover();
    await expect(peek(page)).toHaveAttribute('data-session-id', SESSIONS.shopClaude);
    await page.mouse.move(0, 0);
    await page.mouse.click(1, 400);
    await expect(peek(page)).toHaveCount(0);

    await page.getByTestId('rail-inbox').click();
    await // Billing Claude belongs to a work item: its Now row peeks at that Claude.
    await page.locator(`[data-row="w:0199a6b2-0000-7000-8000-00000000a002"]`).hover();
    await expect(peek(page)).toHaveAttribute('data-session-id', SESSIONS.billingClaude);
  });
});
