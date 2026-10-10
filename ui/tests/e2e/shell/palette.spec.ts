// Command palette, project switcher, Next waiting (attention.next, Mod+J) and the jumplist across projects.
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

  test('Next waiting walks the jump queue across projects', async ({ page }) => {
    await boot(page);
    // First waiting row: Claude replied on the billing scratch item.
    await page.keyboard.press('Control+Shift+J');
    await expect.poll(() => activeProject(page)).toBe('billing');
    await expect(page.getByTestId('jump-hud')).toContainText(/^1\/\d+ · needs input · /);
    // Then the failed start in billing (error band).
    await dispatch(page, 'attention.next');
    await expect(page.getByTestId('jump-hud')).toContainText(/^2\/\d+ · error · /);
    const focused = page.locator('[data-testid="pane"][data-focused="true"]');
    const billingPane = await focused.getAttribute('data-pane-id');
    // Then SHOP-155 to review: its closed work tab is recreated around its Claude session.
    await dispatch(page, 'attention.next');
    await expect.poll(() => activeProject(page)).toBe('shop');
    await expect(page.getByTestId('work-header')).toHaveAttribute('data-phase', 'to_review');
    expect((await callsOf(page, 'session_kill')).length).toBe(0);
    // Back skips shop's old tab, passed through while the work tab was recreated.
    await dispatch(page, 'nav.back');
    await expect.poll(() => activeProject(page)).toBe('billing');
    await expect(focused).toHaveAttribute('data-pane-id', billingPane!);
  });

  test('nav.back returns to the exact pane after Mod+J, kelta-ctl next and kelta-ctl focus-project', async ({
    page,
  }) => {
    await boot(page);
    const focused = page.locator('[data-testid="pane"][data-focused="true"] [data-testid="terminal-pane"]');
    // Focus nvim (not the default pane) so "exact pane" is tested.
    await page.locator(`[data-testid="terminal-pane"][data-session-id="${SESSIONS.shopNvim}"]`).click();
    await expect(focused).toHaveAttribute('data-session-id', SESSIONS.shopNvim);

    const ctl = (cmd: Record<string, unknown>) =>
      page.evaluate((c) => window.__keltaMock!.emit({ type: 'ctl.command', cmd: c as never }), cmd);
    const backToNvim = async () => {
      await expect.poll(() => activeProject(page)).toBe('shop');
      await expect(focused).toHaveAttribute('data-session-id', SESSIONS.shopNvim);
    };

    await page.keyboard.press('Control+Shift+J');
    await expect.poll(() => activeProject(page)).toBe('billing');
    await dispatch(page, 'nav.back');
    await backToNvim();
    await dispatch(page, 'nav.forward');
    await expect.poll(() => activeProject(page)).toBe('billing');
    await dispatch(page, 'nav.back');
    await backToNvim();

    await ctl({ cmd: 'next' });
    await expect(page.getByTestId('jump-hud')).toContainText(/^2\//);
    await expect(focused).not.toHaveAttribute('data-session-id', SESSIONS.shopNvim);
    await ctl({ cmd: 'back' });
    await backToNvim();

    await ctl({ cmd: 'focus_project', id: 'billing' });
    await expect.poll(() => activeProject(page)).toBe('billing');
    await ctl({ cmd: 'back' });
    await backToNvim();
  });
});
