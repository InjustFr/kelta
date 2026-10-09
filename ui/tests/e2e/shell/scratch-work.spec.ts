// Flow 3 on the IPC mock (FLOW §4.3): ⇧⌘N, type the task, ⌘↵ → a focused work tab with Claude;
// the ⌘T hint line; Link to ticket keeps the branch.
import { expect, test, type Page } from '@playwright/test';

import { boot, callsOf, dispatch } from './helpers';

const TASK = 'Fix the login flake\nIt fails on CI about once a day.';

async function newWorkItem(page: Page): Promise<void> {
  await page.keyboard.press('Meta+Shift+N');
  const sheet = page.getByRole('dialog', { name: 'New work item' });
  await expect(sheet).toBeVisible();
  await expect(sheet.locator('textarea')).toBeFocused();
  await page.keyboard.type(TASK);
  await expect(page.getByTestId('new-work').locator('input.mono').first()).toHaveValue(
    'wip/fix-the-login-flake',
  );
  await page.keyboard.press('Meta+Enter');
  await expect(sheet).toBeHidden();
}

test.describe('scratch work items', () => {
  test('⇧⌘N, type, ⌘↵ opens a focused work tab with Claude on the task', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));
    await boot(page, 'macos');
    await newWorkItem(page);

    const [plan] = await callsOf(page, 'work_plan');
    expect(plan?.args?.source).toMatchObject({ kind: 'branch', name: '', task: TASK });
    const active = page.locator('[data-testid="tab"][aria-selected="true"]');
    await expect(active).toContainText('wip Fix the login flake');
    await expect(page.getByTestId('work-key')).toHaveText('wip');
    const claude = await page.evaluate(() => {
      const k = window.__kelta!.stores;
      const item = k.work.all.find((w) => w.branch === 'wip/fix-the-login-flake');
      return k.sessions.all.some((s) => s.work_item_id === item?.id && s.kind.type === 'claude');
    });
    expect(claude).toBe(true);

    // Same first line again: the backend refuses, the sheet stays open with the reason.
    await page.keyboard.press('Meta+Shift+N');
    await page.keyboard.type(TASK);
    await page.keyboard.press('Meta+Enter');
    await expect(page.getByTestId('new-work-error')).toContainText('has a work item');
    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog', { name: 'New work item' })).toBeHidden();
    expect(errors).toEqual([]);
  });

  test('the ⌘T sheet points to New work item', async ({ page }) => {
    await boot(page, 'macos');
    await dispatch(page, 'session.new');
    await expect(page.getByTestId('new-work-hint')).toContainText('Want a branch and a PR?');
    await page.getByTestId('new-work-hint').getByRole('button', { name: 'New work item' }).click();
    await expect(page.getByRole('dialog', { name: 'New work item' })).toBeVisible();
    await expect(page.getByRole('dialog', { name: 'New session' })).toBeHidden();
  });

  test('Link to ticket turns the item into a ticket item and keeps the branch', async ({ page }) => {
    await boot(page, 'macos');
    await newWorkItem(page);
    await dispatch(page, 'work.link');
    await page.getByTestId('ticket-picker-input').fill('SHOP-151');
    await expect(page.getByTestId('ticket-picker-item').first()).toContainText('SHOP-151');
    await page.keyboard.press('Enter');
    await expect(page.getByTestId('link-ticket')).toBeVisible();
    await page.keyboard.press('Meta+Enter');
    await expect(page.getByTestId('toasts')).toContainText('Linked SHOP-151');
    const [link] = await callsOf(page, 'work_link');
    expect(link?.args).toMatchObject({ ticket: { key: 'SHOP-151' }, apply_side_effects: true });
    await expect(page.getByTestId('work-header')).toContainText('SHOP-151');
    await expect(page.getByTestId('work-header')).toContainText('wip/fix-the-login-flake');
  });
});
