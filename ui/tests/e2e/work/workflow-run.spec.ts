// Run a saved workflow on a ticket (#233) on the IPC mock: `w` in the detail, the picker, the run in
// a background tab, the toast's Open and Peek, and the ticket row's run chip.
import { expect, test } from '@playwright/test';

import { boot, callsOf, dispatch } from '../shell/helpers';

test('detail w: the run opens a background tab, the toast opens and peeks its session', async ({ page }) => {
  await boot(page);
  await dispatch(page, 'tickets.open', { project_id: 'shop' });
  await page.getByRole('tab', { name: 'List' }).click(); // the mock layout opens the board
  const row = page.locator('[data-key="jira-acme:SHOP-151"]');
  await row.locator('.k-row-key').click();
  await page.keyboard.press('Enter');
  const detail = page.getByTestId('ticket-detail');
  await expect(detail.getByRole('heading', { level: 1 })).toHaveText('Checkout: show tax breakdown');
  // The opened detail is saved first: a backend layout change drops a pending local save.
  await page.evaluate(() => window.__kelta!.stores.layout.flush());
  await detail.focus();
  await page.keyboard.press('w');
  const picker = page.getByTestId('workflow-picker');
  await expect(picker).toContainText('Implement, review and merge one ticket');
  await expect(picker).toContainText('triage');
  await page.keyboard.press('Enter');

  const toast = page.getByTestId('toasts');
  await expect(toast).toContainText('kelta-ticket-pass started on SHOP-151');
  const [run] = await callsOf(page, 'workflow_run');
  expect(run?.args).toMatchObject({ ticket: { key: 'SHOP-151' }, workflow: 'kelta-ticket-pass' });
  await expect(detail).toBeFocused();
  const tab = page.locator('[data-testid="rail-row"]', { hasText: 'kelta-ticket-pass · SHOP-151' });
  await expect(tab).toHaveCount(1);
  await expect(row).toContainText('kelta-ticket-pass · Starting');

  const sid = await page.evaluate(() => {
    const args = window.__kelta!.stores.toasts.list.at(-1)!.toast.action!.args as { session_id: string };
    return args.session_id;
  });
  await toast.getByRole('button', { name: 'Peek' }).click();
  await expect(page.getByTestId('peek')).toHaveAttribute('data-session-id', sid);
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('peek')).toHaveCount(0);

  // Open (the toast's action, now dismissed) reveals the session's pane.
  await dispatch(page, 'session.reveal', { session_id: sid });
  await expect(
    page.locator(`[data-testid="pane"][data-focused="true"] [data-session-id="${sid}"]`),
  ).toHaveCount(1);
});
