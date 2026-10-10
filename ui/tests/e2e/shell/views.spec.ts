// The rail's cross-project views (SPEC §1, #214): Tickets, Now and Reviews render outside any project.
import { expect, test } from '@playwright/test';

import { activeProject, boot, callsOf } from './helpers';

test('Tickets and Reviews show from the rail, leave for the item project and come back after a restart', async ({
  page,
}) => {
  await boot(page);
  const host = page.getByTestId('global-host');

  // The rail's Tickets row: every project's tickets, the active project unchanged and polled.
  await page.getByTestId('rail-tickets').click();
  await expect(host).toHaveAttribute('data-view', 'tickets');
  await expect(page.getByTestId('rail-tickets')).toHaveAttribute('aria-current', 'page');
  await expect(page.locator('[data-testid="rail-project"][aria-current="page"]')).toHaveCount(0);
  const pane = host.getByTestId('tickets-pane');
  await expect(pane.locator('[data-key]').first()).toBeVisible();
  expect(await activeProject(page)).toBe('shop');
  expect((await callsOf(page, 'ui_set_global_view')).at(-1)?.args).toMatchObject({
    content: { kind: 'tickets', scope: { kind: 'all' }, who: 'mine' },
  });

  // Shift+Enter leaves the view for the ticket's project, in a visible pane.
  await pane.focus();
  await page.keyboard.press('Shift+Enter');
  await expect(host).toBeHidden();
  await expect(page.getByTestId('workspace').getByTestId('ticket-detail')).toBeVisible();
  expect((await callsOf(page, 'ui_set_global_view')).at(-1)?.args).toMatchObject({ content: null });

  // Mod+Shift+R: the Reviews view from a project; Kelta reopens on it.
  await page.keyboard.press('Control+Shift+R');
  await expect(host).toHaveAttribute('data-view', 'reviews');
  await expect(host.getByTestId('reviews-pane')).toBeVisible();
  await page.reload();
  await expect(page.getByTestId('global-host')).toHaveAttribute('data-view', 'reviews');
});

test('p on a ticket lands on its PR in Reviews, and its linked ticket leads back, without opening a project', async ({
  page,
}) => {
  await boot(page);
  const host = page.getByTestId('global-host');
  await page.getByTestId('rail-tickets').click();
  await host.locator('[data-key="redmine-corp:4567"]').click();
  await page.keyboard.press('p');

  // Reviews, on the PR (My PRs), its detail in the column beside the list.
  await expect(host).toHaveAttribute('data-view', 'reviews');
  await expect(host.locator('.k-row.selected')).toHaveAttribute(
    'data-key',
    'gitlab-corp:fin/billing/core#42',
  );
  const detail = host.getByTestId('review-detail');
  await expect(detail).toHaveAttribute('aria-label', 'Review fin/billing/core#42');

  await detail.getByRole('button', { name: 'Open 4567' }).click();
  await expect(host).toHaveAttribute('data-view', 'tickets');
  await expect(host.locator('[data-key="redmine-corp:4567"]')).toHaveClass(/selected/);
  expect(await activeProject(page)).toBe('shop');
});
