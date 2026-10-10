// Ship and Finish on the mock IPC, keyboard only (FLOW §4.5, §4.6, §7.1): the full app shell.
import { expect, test, type Page } from '@playwright/test';

import { boot, callsOf, dispatch, SESSIONS } from '../shell/helpers';

const SHOP_142 = '0199a6b2-0000-7000-8000-00000000a001';
const SHOP_120_MERGED = '0199a6b2-0000-7000-8000-00000000a007';

async function claudeStatus(page: Page, status: string): Promise<void> {
  await page.evaluate(
    ([id, st]) => {
      // The mock's own record (store values are proxies, which do not structured-clone).
      const s = window.__keltaMock!.state.sessions.find((x) => x.id === id)!;
      s.status = st as never;
      window.__keltaMock!.emit({ type: 'session.updated', session: s });
    },
    [SESSIONS.shopClaude, status] as const,
  );
}

test.describe('ship', () => {
  test('refused while Claude works, then ⌘↵ ships and the toast key opens the PR', async ({ page }) => {
    await boot(page);
    const header = page.getByTestId('work-header');
    await header.getByRole('button', { name: 'Ship' }).click();
    const dialog = page.getByRole('dialog', { name: /^Ship SHOP-142/ });
    await expect(dialog).toContainText('Claude is working in this worktree. Ship when it stops.');
    await expect(dialog.getByRole('button', { name: /^Ship/ })).toBeDisabled();
    await page.keyboard.press('Escape');
    await expect(dialog).toHaveCount(0);

    await claudeStatus(page, 'done');
    await dispatch(page, 'work.ship', { id: SHOP_142 });
    await expect(dialog.getByLabel('Title')).toHaveValue('SHOP-142: Rate-limit login attempts');
    await expect(dialog.getByLabel('Title')).toBeFocused();
    await expect(dialog.getByTestId('ship-dirty')).toBeVisible();
    await page.keyboard.press('Control+Enter'); // Ship anyway
    await expect(dialog).toHaveCount(0);
    const toast = page.getByTestId('toasts').getByText('Opened PR #100');
    await expect(toast).toBeVisible();
    // The Run last toast action key is printed on the button.
    const open = page.getByTestId('toasts').getByRole('button', { name: /Open/ });
    await expect(open.locator('kbd').first()).toBeVisible();
    await page.keyboard.press('Control+Shift+A');
    await expect.poll(async () => (await callsOf(page, 'open_external')).length).toBe(1);
    expect((await callsOf(page, 'open_external'))[0]?.args).toEqual({
      url: 'https://github.com/acme/mock/pull/100',
    });
    await expect(header.getByRole('button', { name: 'Open PR' })).toBeVisible();
  });
});

test.describe('finish', () => {
  test('a merged item finishes from a prefilled dialog with ⌘↵', async ({ page }) => {
    await boot(page);
    await dispatch(page, 'work.finish', { id: SHOP_120_MERGED });
    const dialog = page.getByRole('dialog', { name: /^Finish feat\/SHOP-120/ });
    await expect(dialog.getByRole('switch', { name: 'Remove the worktree' })).toBeChecked();
    await expect(dialog.getByRole('switch', { name: 'Delete the local branch' })).toBeChecked();
    // Space toggles the focused switch; Tab moves on.
    await expect(dialog.getByRole('switch', { name: 'Remove the worktree' })).toBeFocused();
    await page.keyboard.press('Tab');
    await page.keyboard.press('Space');
    await expect(dialog.getByRole('switch', { name: 'Delete the local branch' })).not.toBeChecked();
    await page.keyboard.press('Space');
    await page.keyboard.press('Control+Enter');
    await expect(dialog).toHaveCount(0);
    await expect(page.getByTestId('toasts')).toContainText('Finished feat/SHOP-120-upgrade-payment-sdk');
    expect((await callsOf(page, 'work_finish'))[0]?.args).toMatchObject({
      id: SHOP_120_MERGED,
      opts: { remove_worktree: true, delete_branch: true, force: false, transition_to: null },
    });
  });

  test('Finish all merged from the palette skips dirty worktrees', async ({ page }) => {
    await boot(page);
    await page.keyboard.press('Control+Shift+K');
    await page.getByTestId('palette-input').fill('finish all merged');
    await expect(page.getByTestId('palette-item').first()).toContainText('Finish all merged');
    await page.keyboard.press('Enter');
    const dialog = page.getByRole('dialog', { name: 'Finish all merged' });
    await expect(dialog.getByRole('list', { name: 'To finish' })).toContainText('SHOP-120');
    const skipped = dialog.getByRole('list', { name: 'Skipped' });
    await expect(skipped).toContainText('wip/cache-warmup');
    await expect(skipped).toContainText('uncommitted changes');
    await page.keyboard.press('Control+Enter');
    await expect(dialog).toHaveCount(0);
    await expect(page.getByTestId('toasts')).toContainText('Finished 1 merged work item, 2 skipped');
    expect((await callsOf(page, 'work_finish_merged')).length).toBe(1);
  });
});
