// Project colours (#253): a picked chip stores `theme:<slot>` and follows the theme; a custom hex stays put.
import { expect, test, type Page } from '@playwright/test';

import { boot, callsOf, dispatch } from '../shell/helpers';

test.use({ colorScheme: 'dark' });

const rgb = (hex: string): string =>
  `rgb(${[1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)).join(', ')})`;

/** `--project-color` of a project's rail row. */
const bar = (page: Page, id: string) =>
  page
    .locator(`[data-testid="rail-project"][data-project-id="${id}"]`)
    .evaluate((el) => (el.closest('[data-testid="rail-band"]') as HTMLElement).style.getPropertyValue('--project-color'));

const accent = (page: Page) =>
  page.evaluate(() => document.documentElement.style.getPropertyValue('--k-project'));

async function pickTheme(page: Page, id: string): Promise<void> {
  await dispatch(page, 'settings.open', { section: 'general' });
  await page
    .getByRole('listbox', { name: /Dark theme/ })
    .locator(`[data-theme-id="${id}"]`)
    .click();
}

async function editProject(page: Page, name: string): Promise<void> {
  await dispatch(page, 'settings.open', { section: 'projects' });
  await page.getByRole('button', { name: `Edit ${name}` }).click();
}

test('a picked theme colour follows the theme, a custom hex stays as typed', async ({ page }) => {
  await boot(page);

  // Bezel Dark: the picker shows the theme's colours; picking Green stores the slot.
  await editProject(page, 'Shop');
  const green = page.getByRole('button', { name: 'Green', exact: true });
  await expect(page.locator('[data-slot]')).toHaveCount(12);
  await expect(green).toHaveCSS('background-color', rgb('#79b88a'));
  await green.click();
  await page.getByRole('button', { name: 'Save' }).click();
  await expect
    .poll(async () => (await callsOf(page, 'project_update')).at(-1)?.args)
    .toMatchObject({
      id: 'shop',
      patch: { color: 'theme:green' },
    });
  await expect.poll(() => bar(page, 'shop')).toBe('#79b88a');
  await expect.poll(() => accent(page)).toBe('#79b88a');

  // A custom hex on Billing.
  await editProject(page, 'Billing');
  await page.getByLabel('Custom colour (hex)').fill('#ff8800');
  await page.getByLabel('Custom colour (hex)').blur();
  await expect(page.getByTestId('custom-color')).toHaveAttribute('title', 'Custom #ff8800');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect.poll(() => bar(page, 'billing')).toBe('#ff8800');

  for (const [theme, hex] of [
    ['gruvbox-dark', '#98971a'],
    ['dracula', '#50fa7b'],
    ['bezel-dark', '#79b88a'],
  ] as const) {
    await pickTheme(page, theme);
    await expect.poll(() => bar(page, 'shop')).toBe(hex);
    await expect.poll(() => accent(page)).toBe(hex);
    await expect.poll(() => bar(page, 'billing')).toBe('#ff8800');
  }

  // Nord: the chips are Nord's, the slot stays selected, the custom hex keeps its chip.
  await pickTheme(page, 'nord');
  await editProject(page, 'Shop');
  await expect(page.getByRole('button', { name: 'Green', exact: true })).toHaveCSS(
    'background-color',
    rgb('#a3be8c'),
  );
  await expect(page.getByRole('button', { name: 'Green', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await page.getByRole('button', { name: 'Cancel' }).click();
  await editProject(page, 'Billing');
  await expect(page.getByTestId('custom-color')).toHaveAttribute('title', 'Custom #ff8800');
  await expect(page.getByLabel('Custom colour (hex)')).toHaveValue('#ff8800');
});
