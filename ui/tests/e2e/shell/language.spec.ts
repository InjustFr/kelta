// Interface language (#344): the OS language by default, Settings → General → Language switches it
// without a reload, and `system` goes back to the OS language.
import { expect, test } from '@playwright/test';

import { boot, dispatch } from './helpers';

test.describe('interface language', () => {
  test.describe('on a German OS', () => {
    test.use({ locale: 'de-DE' });

    test('the interface follows the OS language', async ({ page }) => {
      await boot(page);
      await expect(page.locator('html')).toHaveAttribute('lang', 'de');
      await expect(page.getByTestId('rail')).toContainText('Einstellungen');
      await expect(page.getByTestId('context-search')).toContainText('Suchen oder ausführen…');
    });
  });

  test('Settings → General switches the language at once, and back to the OS one', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));
    await boot(page);
    await expect(page.getByTestId('rail')).toContainText('Settings');

    await dispatch(page, 'settings.open', { section: 'general' });
    const language = page.locator('[data-testid="field"][data-path="app.language"]').getByRole('combobox');
    await language.selectOption('fr');

    await expect(page.locator('html')).toHaveAttribute('lang', 'fr');
    await expect(page.getByTestId('rail')).toContainText('Paramètres');
    await expect(page.getByRole('navigation', { name: 'Sections des paramètres' })).toContainText('Général');

    await language.selectOption('de');
    await expect(page.getByTestId('rail')).toContainText('Einstellungen');

    await language.selectOption('system');
    await expect(page.locator('html')).toHaveAttribute('lang', 'en');
    await expect(page.getByTestId('rail')).toContainText('Settings');
    expect(errors).toEqual([]);
  });
});
