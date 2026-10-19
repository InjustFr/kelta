// The window's top bar (#248, #429): one 30px bar, `‹ ›`, the breadcrumb (none on a work tab, whose
// header above its panes names it, #428), the search field opening the palette, the project scope, the
// "needs you" chip and `+ New`; the window title follows the breadcrumb (native decorations on mock
// IPC), and narrow windows drop items in the stated order.
import { expect, test, type Page } from '@playwright/test';

import { boot, dispatch, unfold } from './helpers';

const bar = (page: Page) => page.getByTestId('contextbar');

test('one 30px bar; the window title follows the breadcrumb, none on a work tab', async ({ page }) => {
  await boot(page);
  expect((await bar(page).boundingBox())?.height).toBe(30);
  await expect(page.getByTestId('titlebar')).toHaveCount(0);
  await expect(page).toHaveTitle(/^Shop › SHOP-142 Rate-limit login/);
  await expect(bar(page).getByTestId('context-crumb')).toHaveCount(0);
  await unfold(page, 'shop');
  await page.locator('[data-testid="rail-row"][data-tab-id="shop-t2"]').click();
  await expect(page).toHaveTitle('Shop › ~/code/shop-api');
  await expect(bar(page).getByTestId('context-crumb')).toHaveText(/Shop\s*›\s*❯\s*~\/code\/shop-api/);
  await page.getByTestId('rail-inbox').click();
  await expect(page).toHaveTitle('Queue');
  await expect(bar(page).getByTestId('context-crumb')).toContainText('Queue');
  await expect(bar(page).getByTestId('context-underneath')).toContainText('Shop › Shell');
});

test('the search field opens the palette; the rail has no search row', async ({ page }) => {
  await boot(page);
  await expect(page.getByTestId('rail-search')).toHaveCount(0);
  await bar(page).getByTestId('context-search').click();
  // A load, not an assertion: the palette chunk is lazy and takes seconds on a loaded machine (#485).
  await page.getByTestId('palette-input').waitFor();
  await expect(page.getByTestId('palette-input')).toBeFocused();
});

test('‹ › walk the jumplist; + New offers a work item and a session', async ({ page }) => {
  await boot(page);
  await page.getByTestId('rail-reviews').click();
  await expect(page).toHaveTitle('Pull requests');
  await bar(page).getByTestId('context-back').click();
  await expect(page).toHaveTitle(/^Shop › SHOP-142/);
  await bar(page).getByTestId('context-forward').click();
  await expect(page).toHaveTitle('Pull requests');

  await bar(page).getByTestId('tab-plus').click();
  await expect(page.getByRole('menuitem')).toHaveText([/New work item/, /New session/]);
});

test('⌘0 ⌘0 comes back to the focused pane; Esc on the Queue clears its filter, then goes back (#415)', async ({
  page,
}) => {
  await boot(page);
  const pane = page.locator('[data-testid="workspace"] [data-testid="pane"]').nth(1);
  const filter = page.getByLabel('Filter Queue');
  const backHere = async () => {
    await expect(page).toHaveTitle(/^Shop › SHOP-142/);
    await expect(pane).toHaveAttribute('data-focused', 'true');
    await expect(pane.locator('.xterm-helper-textarea')).toBeFocused();
  };
  await dispatch(page, 'pane.focus_right');
  await expect(pane).toHaveAttribute('data-focused', 'true');

  await page.keyboard.press('Control+0');
  await expect(page).toHaveTitle('Queue');
  await expect(bar(page).getByTestId('context-back')).toHaveAttribute('aria-label', /^Back to Shop › /);
  await page.keyboard.press('Control+0');
  await backHere();

  await page.keyboard.press('Control+0');
  await page.getByTestId('inbox-pane').focus();
  await page.keyboard.press('/');
  await filter.fill('zzz');
  await filter.press('Enter');
  await page.keyboard.press('Escape');
  await expect(filter).toHaveValue('');
  await expect(page).toHaveTitle('Queue');
  await page.keyboard.press('Escape');
  await backHere();
});

test('a work tab has its header above the panes, and keeps it over a zoomed pane', async ({ page }) => {
  await boot(page);
  const header = page.getByTestId('work-header');
  await expect(header).toBeVisible();
  await expect(bar(page).getByTestId('work-header')).toHaveCount(0);
  const top = (await header.boundingBox())!;
  const workPanes = (await page.getByTestId('panes').boundingBox())!;
  expect(workPanes.y).toBeGreaterThanOrEqual(top.y + top.height);
  await page.keyboard.press('Control+Shift+Enter'); // pane.zoom
  await expect(header).toBeVisible();
  await unfold(page, 'shop');
  await page.locator('[data-testid="rail-row"][data-tab-id="shop-t2"]').click();
  await expect(header).toHaveCount(0);
  const shellPanes = (await page.getByTestId('panes').boundingBox())!;
  expect(shellPanes.y).toBeLessThan(workPanes.y);
});

test('narrow windows drop the search text; ‹ ›, the search icon, the scope and + New stay', async ({
  page,
}) => {
  await boot(page);
  const at = async (width: number) => page.setViewportSize({ width, height: 800 });
  const text = bar(page).locator('.search-text');

  await at(1400);
  await expect(text).toBeVisible();
  await at(900);
  await expect(text).toBeVisible();
  await at(820);
  await expect(text).toBeHidden();
  await at(640);
  for (const id of ['context-back', 'context-forward', 'context-search', 'list-scope', 'tab-plus'])
    await expect(bar(page).getByTestId(id)).toBeInViewport({ ratio: 1 });
});
