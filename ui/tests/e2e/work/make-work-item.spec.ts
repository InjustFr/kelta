// Make this a work item (#502, FLOW §4.3) on the IPC mock: a plain Claude tab becomes a ticket item or a
// scratch item, from the palette action or the sidebar row menu, and is not offered on a session in an item.
import { expect, test, type Page } from '@playwright/test';

import { boot, callsOf, dispatch, SESSIONS } from '../shell/helpers';

/** ⌘T, Enter (Claude and the editor, a new tab), then the id of the new Claude: the only one in no item. */
async function plainClaude(page: Page): Promise<string> {
  await dispatch(page, 'session.new');
  await page.getByTestId('new-session-start').click();
  await page
    .locator(
      '[data-testid="workspace"] [data-testid="pane"][data-focused="true"] [data-testid="terminal-pane"][data-attached="true"]',
    )
    .waitFor();
  return page.evaluate(() => {
    const k = window.__kelta!.stores;
    return k.sessions.all.find((s) => s.kind.type === 'claude' && s.lifecycle === 'live' && !s.work_item_id)!
      .id;
  });
}

const itemOf = (page: Page, sid: string) =>
  page.evaluate((id) => {
    const k = window.__kelta!.stores;
    return k.work.all.find((w) => w.session_ids.includes(id)) ?? null;
  }, sid);

test('with a ticket, the plain Claude becomes the claimed item’s Claude', async ({ page }) => {
  await boot(page, 'macos');
  const sid = await plainClaude(page);
  await dispatch(page, 'session.make_work');
  const sheet = page.getByTestId('make-work');
  await sheet.getByRole('button', { name: 'None. Link one…' }).click();
  await page.getByTestId('ticket-picker-input').fill('SHOP-151');
  await page.keyboard.press('Enter');
  await expect(page.getByTestId('make-work-ticket')).toContainText('SHOP-151');
  await page.getByTestId('make-work-submit').click();
  await expect(sheet).toBeHidden();

  const [call] = await callsOf(page, 'session_claim');
  expect(call?.args).toMatchObject({ id: sid, ticket: { key: 'SHOP-151' } });
  const item = await itemOf(page, sid);
  expect(item).toMatchObject({ kind: 'ticket', session_ids: [sid], run: { session_id: sid } });
  expect(await page.evaluate((id) => window.__kelta!.stores.sessions.get(id)?.work_item_id, sid)).toBe(
    item?.id,
  );
  await expect(page.getByTestId('work-header-key')).toHaveText('SHOP-151');
  await expect(page.locator(`[data-testid="pane"][data-focused="true"] [data-session-id="${sid}"]`)).toHaveCount(1);
});

test('without a ticket, from the sidebar row menu, it makes a scratch item', async ({ page }) => {
  await boot(page, 'macos');
  const sid = await plainClaude(page);
  await page.locator(`[data-testid="rail-row"][data-session-id="${sid}"]`).click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Make this a work item' }).click();
  await page.getByTestId('make-work-submit').click();
  await expect(page.getByTestId('make-work')).toBeHidden();

  const [call] = await callsOf(page, 'session_claim');
  expect(call?.args).toMatchObject({ id: sid, ticket: null });
  expect(await itemOf(page, sid)).toMatchObject({ kind: 'branch', session_ids: [sid], run: null });
});

test('not offered on a Claude in a work item, nor on a shell', async ({ page }) => {
  await boot(page, 'macos');
  await page.locator('[data-testid="rail-row"][data-work-id]').first().click({ button: 'right' });
  await expect(page.getByRole('menuitem', { name: 'Close' })).toBeVisible();
  await expect(page.getByRole('menuitem', { name: 'Make this a work item' })).toHaveCount(0);
  await page.keyboard.press('Escape');
  for (const id of [SESSIONS.shopClaude, '0199a6b2-0000-7000-8000-000000000003']) {
    await dispatch(page, 'session.make_work', { session_id: id });
    await expect(page.getByTestId('make-work')).toHaveCount(0);
    await expect(page.getByTestId('toasts')).toContainText('Focus a Claude session that is in no work item');
  }
});
