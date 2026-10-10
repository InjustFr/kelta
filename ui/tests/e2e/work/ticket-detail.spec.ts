// Ticket detail and status picker (TICKETS.md T2/T3) on the IPC mock: action bar keys, the picker's
// filter + Enter, and the palette's "Move KEY to…" with a move the tracker refuses.
import { expect, test } from '@playwright/test';

import { boot, callsOf, dispatch } from '../shell/helpers';

test('detail: m opens the status picker, typing filters, Enter moves', async ({ page }) => {
  await boot(page);
  await dispatch(page, 'tickets.open', { project_id: 'shop' });
  await page.locator('[data-key="jira-acme:SHOP-151"]').click();
  await page.keyboard.press('Enter');
  const detail = page.getByTestId('ticket-detail');
  await expect(detail.getByRole('heading', { level: 1 })).toHaveText('Checkout: show tax breakdown');
  await expect(detail.getByRole('toolbar', { name: 'Ticket actions' })).toContainText('Start work');
  await detail.focus();
  await page.keyboard.press('m');
  await expect(page.getByRole('menuitem')).toHaveCount(4);
  await page.keyboard.type('rev');
  await expect(page.getByRole('menuitem')).toHaveCount(1);
  await page.keyboard.press('Enter');
  await expect(detail.getByRole('button', { name: /^Status In review/ })).toBeVisible();
  expect((await callsOf(page, 'tracker_transition')).at(-1)?.args).toMatchObject({
    transition_id: 'to-in_review',
  });
});

test('palette move refused by the tracker: its message and Open in browser', async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const t = window.__keltaMock!.state.tickets.find((i) => i.ticket.ref.key === '4567')!;
    window.__kelta!.stores.ui.openSheet('tickets.move', { ticket: t.ticket, project_id: 'shop' });
  });
  await expect(page.getByRole('menu', { name: 'Move 4567' })).toBeVisible();
  await page.keyboard.press('2'); // Resolved
  const toast = page.getByTestId('toasts');
  await expect(toast).toContainText('Redmine: status transition not allowed (422)');
  await toast.getByRole('button', { name: 'Open in browser' }).click();
  await expect.poll(async () => (await callsOf(page, 'open_external')).length).toBe(1);
});
