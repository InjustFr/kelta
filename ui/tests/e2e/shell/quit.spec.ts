// Quit dialog (#305): keltad keeps sessions running past the quit unless the user stops them.
import { expect, type Page, test } from '@playwright/test';

import { boot, callsOf, dispatch } from './helpers';

const requestQuit = (page: Page) =>
  page.evaluate(() => window.__keltaMock!.emit({ type: 'app.quit_requested', live: 2, busy: ['claude'] }));

test.describe('quit dialog', () => {
  test('Cancel keeps Kelta open', async ({ page }) => {
    await boot(page);
    await requestQuit(page);
    const dialog = page.getByTestId('confirm-dialog');
    await expect(dialog).toContainText('2 sessions are running');
    await expect(dialog).toContainText('claude is busy');
    await page.getByRole('button', { name: 'Cancel' }).click();
    await expect(dialog).toHaveCount(0);
    expect(await callsOf(page, 'app_quit')).toEqual([]);
  });

  test("Stop all sessions with Don't ask again stores the choice and quits", async ({ page }) => {
    await boot(page);
    await requestQuit(page);
    await expect(page.getByTestId('confirm-keep')).toHaveText('Keep sessions running');
    await page.getByTestId('confirm-checkbox').getByText("Don't ask again").click();
    await page.getByTestId('confirm-stop').click();
    await expect
      .poll(async () => (await callsOf(page, 'app_quit')).map((c) => c.args))
      .toEqual([{ stop: true }]);
    const set = (await callsOf(page, 'settings_set')).map((c) => c.args);
    expect(set).toContainEqual({ layer: 'global', path: 'app.quit_sessions', value: 'stop' });

    await dispatch(page, 'settings.open', { section: 'general' });
    await expect(page.getByLabel('Sessions on quit', { exact: true })).toHaveValue('stop');
  });
});
