// Settings → General theme picker (#229): picking a card re-themes the window without a reload.
import { expect, test } from '@playwright/test';

import { boot, callsOf, dispatch } from '../shell/helpers';

test.use({ colorScheme: 'dark' });

test('picking a dark theme with the keyboard re-themes the window', async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const mock = window.__keltaMock!;
    (mock.state.settings.value as { themes: Record<string, unknown> }).themes = {
      sol: { name: 'Solarized', base: 'dark', ui: { well: '#002b36', fg: '#93a1a1' }, terminal: {} },
    };
    mock.emit({ type: 'settings.changed', layers: ['global'], paths: ['themes'], requires_restart: [] });
  });
  await dispatch(page, 'settings.open', { section: 'general' });

  const dark = page.getByRole('listbox', { name: /Dark theme/ });
  await expect(dark.getByRole('option')).toHaveCount(2);
  await expect(page.locator('[data-testid="field"][data-path="app.dark_theme"]')).toHaveCount(0);
  await expect(page.locator('[data-testid="field"][data-path="app.theme"]')).toBeVisible();

  const well = () => page.evaluate(() => document.documentElement.style.getPropertyValue('--k-well'));
  expect(await well()).toBe('');
  await dark.getByRole('option', { selected: true }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(dark.locator('[data-theme-id="sol"]')).toBeFocused();
  expect(await callsOf(page, 'settings_set')).toHaveLength(0);

  await page.keyboard.press('Enter');
  await expect(dark.locator('[data-theme-id="sol"]')).toHaveAttribute('aria-selected', 'true');
  await expect.poll(well).toBe('#002b36');

  // Tab leaves the dark grid for the light one.
  await page.keyboard.press('Tab');
  await expect(
    page.getByRole('listbox', { name: /Light theme/ }).getByRole('option', { selected: true }),
  ).toBeFocused();

  // The slots are global-only: other layers get a note instead of the picker.
  await page.getByRole('button', { name: 'Project', exact: true }).click();
  await expect(page.getByTestId('theme-note')).toHaveText('Themes are chosen in global settings.');
  await expect(page.getByTestId('theme-picker')).toHaveCount(0);
});
