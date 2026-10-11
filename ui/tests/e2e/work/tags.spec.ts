// Local tags (#349) on the IPC mock: created in Settings, put on a ticket and a PR from their rows,
// shown as chips, filtered by from the toolbar; tagging only calls `tags_set`.
import { expect, test } from '@playwright/test';

import { boot, callLog, dispatch } from '../shell/helpers';

test('a tag made in Settings goes on a ticket and a PR and filters the list', async ({ page }) => {
  await boot(page);
  await dispatch(page, 'settings.open', { section: 'tags' });
  await page.getByRole('button', { name: 'New tag' }).click();
  const form = page.getByTestId('tag-new');
  await form.getByLabel('Name').fill('retest');
  await form.locator('[data-slot="red"]').click();
  await form.getByRole('button', { name: 'Save' }).click();
  await expect(page.locator('[data-testid="tag-row"][data-tag="retest"]')).toBeVisible();

  await dispatch(page, 'tickets.open', { project_id: 'shop' });
  await page.getByRole('tab', { name: 'List' }).click();
  const row = page.locator('[data-key="jira-acme:SHOP-142"]');
  await row.locator('.k-row-key').click();
  const before = (await callLog(page)).length;
  await page.keyboard.press('t');
  await page.getByRole('menu', { name: 'Tags' }).getByRole('menuitem', { name: 'retest' }).click();
  await expect(row.locator('[data-tag="retest"]')).toBeVisible();
  expect((await callLog(page)).slice(before).map((c) => c.cmd)).toEqual(['tags_set']);

  const tickets = page.getByTestId('tickets-pane');
  await expect(tickets.locator('.k-row.tk')).not.toHaveCount(1);
  await tickets
    .getByRole('group', { name: 'Filter by tag' })
    .getByRole('button', { name: /retest/ })
    .click();
  await expect(tickets.locator('.k-row.tk')).toHaveCount(1);
  await expect(tickets.locator('.k-row.tk')).toHaveAttribute('data-key', 'jira-acme:SHOP-142');

  await dispatch(page, 'reviews.open');
  const reviews = page.getByTestId('reviews-pane');
  const pr = reviews.locator('.k-row.selected');
  await pr.click();
  await page.keyboard.press('t');
  await page.getByRole('menu', { name: 'Tags' }).getByRole('menuitem', { name: 'retest' }).click();
  await expect(pr.locator('[data-tag="retest"]')).toBeVisible();
  await reviews
    .getByRole('group', { name: 'Filter by tag' })
    .getByRole('button', { name: /retest/ })
    .click();
  await expect(reviews.locator('.k-row')).toHaveCount(1);
});
