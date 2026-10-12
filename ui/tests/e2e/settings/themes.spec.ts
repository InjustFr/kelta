// Settings → General theme picker (#229): picking a card re-themes the window without a reload.
import { expect, test } from '@playwright/test';

import { boot, callsOf, dispatch } from '../shell/helpers';

test.use({ colorScheme: 'dark' });

test('picking a built-in dark theme with the keyboard re-themes the window', async ({ page }) => {
  await boot(page);
  await dispatch(page, 'settings.open', { section: 'general' });

  const dark = page.getByRole('listbox', { name: /Dark theme/ });
  await expect(dark.getByRole('option')).toHaveCount(6);
  await expect(page.locator('[data-testid="field"][data-path="app.dark_theme"]')).toHaveCount(0);
  await expect(page.locator('[data-testid="field"][data-path="app.theme"]')).toBeVisible();

  const well = () => page.evaluate(() => document.documentElement.style.getPropertyValue('--k-well'));
  expect(await well()).toBe('');
  await dark.getByRole('option', { selected: true }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(dark.locator('[data-theme-id="solarized-dark"]')).toBeFocused();
  expect(await callsOf(page, 'settings_set')).toHaveLength(0);

  await page.keyboard.press('Enter');
  await expect(dark.locator('[data-theme-id="solarized-dark"]')).toHaveAttribute('aria-selected', 'true');
  await expect.poll(well).toBe('#002b36');
  expect(
    await page.evaluate(() => document.documentElement.style.getPropertyValue('--k-term-selection')),
  ).toBe('#073642');

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

test('importing a VS Code theme adds its card, selects it and re-themes the window', async ({ page }) => {
  await boot(page);
  await dispatch(page, 'settings.open', { section: 'general' });
  const theme = {
    name: 'Night Owl',
    type: 'dark',
    colors: { 'editor.background': '#011627', 'editor.foreground': '#d6deeb' },
  };
  await page.getByTestId('theme-import-input').setInputFiles({
    name: 'night-owl-color-theme.json',
    mimeType: 'application/json',
    buffer: Buffer.from(`// exported\n${JSON.stringify(theme)}`),
  });

  const card = page.getByRole('listbox', { name: /Dark theme/ }).locator('[data-theme-id="night-owl"]');
  await expect(card).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByText('Imported Night Owl (used in dark mode)')).toBeVisible();
  await expect
    .poll(() => page.evaluate(() => document.documentElement.style.getPropertyValue('--k-well')))
    .toBe('#011627');
});

test('an imported theme with one flat surface keeps its steps and says what it raised', async ({ page }) => {
  await boot(page);
  await dispatch(page, 'settings.open', { section: 'general' });
  const flat = '#212337';
  const theme = {
    name: 'Flat Moon',
    type: 'dark',
    colors: {
      'editor.background': flat,
      'editor.foreground': '#d8dbf3',
      'sideBar.background': flat,
      'tab.inactiveBackground': flat,
      'tab.inactiveForeground': '#5b5e75',
    },
  };
  await page.getByTestId('theme-import-input').setInputFiles({
    name: 'flat-moon-color-theme.json',
    mimeType: 'application/json',
    buffer: Buffer.from(JSON.stringify(theme)),
  });

  const card = page.getByRole('listbox', { name: /Dark theme/ }).locator('[data-theme-id="flat-moon"]');
  await expect(card).toHaveAttribute('aria-selected', 'true');
  await expect(card.getByTestId('theme-note')).toHaveText([
    'Surface steps derived from the background',
    'Subtle text raised to 4.5:1',
    'Strong border raised to 3:1',
  ]);
  const css = (name: string) =>
    page.evaluate((n) => document.documentElement.style.getPropertyValue(n), name);
  await expect.poll(() => css('--k-well')).toBe(flat);
  expect(await css('--k-bezel')).not.toBe(flat);
  expect(await css('--k-fg-subtle')).not.toBe('#5b5e75');
});
