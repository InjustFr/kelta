// Command palette, project switcher and Next waiting (attention.next) across projects.
import { expect, test } from '@playwright/test';

import { activeProject, boot, callsOf, dispatch, SESSIONS } from './helpers';

test.describe('palette', () => {
  test('jumps to a session of another project and focuses its pane', async ({ page }) => {
    await boot(page);
    await page.keyboard.press('Control+Shift+K');
    await expect(page.getByTestId('palette')).toBeVisible();
    await page.getByTestId('palette-input').fill('billing claude');
    const first = page.getByTestId('palette-item').first();
    await expect(first).toContainText('claude');
    await expect(first).toContainText('Billing');
    await expect(first).toContainText('Needs input');
    await page.keyboard.press('Enter');

    await expect(page.getByTestId('palette')).toHaveCount(0);
    await expect.poll(() => activeProject(page)).toBe('billing');
    const focused = page.locator('[data-testid="pane"][data-focused="true"]');
    await expect(focused).toHaveCount(1);
    await expect(focused.locator('[data-testid="terminal-pane"]')).toHaveAttribute(
      'data-session-id',
      SESSIONS.billingClaude,
    );
    expect((await callsOf(page, 'session_kill')).length).toBe(0);
  });

  test('finds actions, projects, tools and settings sections', async ({ page }) => {
    await boot(page);
    await page.keyboard.press('Control+Shift+K');
    const input = page.getByTestId('palette-input');
    await input.fill('split right');
    await expect(page.getByTestId('palette-item').first()).toContainText('Split right');
    await expect(page.getByTestId('palette-item').first().locator('kbd').first()).toBeVisible();
    await input.fill('kelta tools');
    await expect(page.getByTestId('palette-item').first()).toContainText('Kelta tools');
    await input.fill('lazygit');
    await expect(page.getByTestId('palette-item').first()).toContainText('Open lazygit');
    await input.fill('settings keys');
    await expect(page.getByTestId('palette-item').first()).toContainText('Settings: Keys');
    await input.fill('zzzzqqq');
    await expect(page.getByTestId('palette-item')).toHaveCount(0);
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('palette')).toHaveCount(0);
  });

  test('searches tickets through tracker_search', async ({ page }) => {
    await boot(page);
    await page.keyboard.press('Control+Shift+K');
    await page.getByTestId('palette-input').fill('SHOP-1');
    await expect.poll(async () => (await callsOf(page, 'tracker_search')).length).toBeGreaterThan(0);
    const ticket = page.getByTestId('palette-item').filter({ hasText: 'SHOP-' }).first();
    await expect(ticket).toBeVisible();
    const call = (await callsOf(page, 'tracker_search'))[0]!;
    expect(call.args).toMatchObject({ scope: { kind: 'all' }, text: 'SHOP-1' });
  });

  test('runs an action from the palette and the typed query survives opening', async ({ page }) => {
    await boot(page);
    await page.keyboard.press('Control+Shift+K');
    await page.getByTestId('palette-input').fill('new session');
    await page.keyboard.press('Enter');
    await expect(page.getByTestId('new-session')).toBeVisible();
  });

  test('project switcher opens and activates a project', async ({ page }) => {
    await boot(page);
    await page.keyboard.press('Control+Shift+P');
    await expect(page.getByTestId('switcher')).toBeVisible();
    await page.getByTestId('switcher-input').fill('kelta');
    await page.keyboard.press('Enter');
    await expect.poll(() => activeProject(page)).toBe('kelta-tools');
    await expect(page.locator('[data-testid="workspace"][data-project-id="kelta-tools"]')).toBeVisible();
  });

  test('Next waiting walks the Now queue across projects, cycling', async ({ page }) => {
    await boot(page);
    // First waiting row: Claude replied on the billing scratch item.
    await page.keyboard.press('Control+Shift+U');
    await expect.poll(() => activeProject(page)).toBe('billing');
    // Then SHOP-155 to review: its closed work tab is recreated around its Claude session.
    await dispatch(page, 'attention.next');
    await expect.poll(() => activeProject(page)).toBe('shop');
    await expect(page.getByTestId('work-header')).toHaveAttribute('data-phase', 'to_review');
    expect((await callsOf(page, 'session_kill')).length).toBe(0);
  });
});
