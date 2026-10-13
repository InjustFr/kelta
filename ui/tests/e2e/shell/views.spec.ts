// The rail's cross-project views (SPEC §1, #214): Tickets, Now and Reviews render outside any project.
import { expect, test, type Page } from '@playwright/test';

import type { Layout } from '$lib/gen';

import { activeProject, boot, callsOf } from './helpers';

/** The last save of the global views' built-in layout: its active tab is the view the core polls. */
async function globalSave(page: Page): Promise<unknown> {
  const saves = await callsOf(page, 'layout_save');
  return saves.filter((c) => (c.args as { layout: Layout }).layout.project_id === 'global').at(-1)?.args;
}

test('Tickets and Reviews show from the rail, leave for the item project and come back after a restart', async ({
  page,
}) => {
  await boot(page);
  const host = page.getByTestId('global-host');

  // The rail's Tickets row: every project's tickets, the active project unchanged and polled.
  await page.getByTestId('rail-tickets').click();
  await expect(host).toHaveAttribute('data-view', 'tickets');
  await expect(page.getByTestId('rail-tickets')).toHaveAttribute('aria-current', 'page');
  await expect(page.locator('[data-testid="rail-row"][aria-current="page"]')).toHaveCount(0);
  const pane = host.getByTestId('tickets-pane');
  await expect(pane.locator('[data-key]').first()).toBeVisible();
  expect(await activeProject(page)).toBe('shop');
  await expect.poll(() => globalSave(page)).toMatchObject({ layout: { active_tab: 'tickets' } });

  // Shift+Enter leaves the view for the ticket's project, in a visible pane.
  await pane.focus();
  await page.keyboard.press('Shift+Enter');
  await expect(host).toBeHidden();
  await expect(page.getByTestId('workspace').getByTestId('ticket-detail')).toBeVisible();
  await expect.poll(() => globalSave(page)).toMatchObject({ layout: { active_tab: null } });

  // Mod+Shift+R: the Reviews view from a project; Kelta reopens on it.
  await page.keyboard.press('Control+Shift+R');
  await expect(host).toHaveAttribute('data-view', 'reviews');
  await expect(host.getByTestId('reviews-pane')).toBeVisible();
  await expect.poll(() => globalSave(page)).toMatchObject({ layout: { active_tab: 'reviews' } });
  await page.reload();
  await expect(page.getByTestId('global-host')).toHaveAttribute('data-view', 'reviews');
});

test('p on a ticket lands on its PR in Reviews, and its linked ticket leads back, without opening a project', async ({
  page,
}) => {
  await boot(page);
  const host = page.getByTestId('global-host');
  await page.getByTestId('rail-tickets').click();
  // The key cell: the row's middle can sit under its hover actions.
  await host.locator('[data-key="redmine-corp:4567"] .k-row-key').click();
  await page.keyboard.press('p');

  // Reviews, on the PR (Mine), its detail in the column beside the list.
  await expect(host).toHaveAttribute('data-view', 'reviews');
  await expect(host.locator('[data-testid="reviews-pane"] .row.selected')).toHaveAttribute(
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

test('Now, Reviews and Tickets fill the window: their hint bar sits at the bottom, even above an empty state', async ({
  page,
}) => {
  await boot(page);
  const host = page.getByTestId('global-host');
  for (const height of [900, 600]) {
    await page.setViewportSize({ width: 1440, height });
    for (const view of ['inbox', 'reviews', 'tickets']) {
      await page.getByTestId(`rail-${view}`).click();
      const pane = host.getByTestId(`${view}-pane`);
      // Now has no footer key bar (#399): its pane itself must reach the bottom.
      const foot = view === 'inbox' ? pane : pane.locator('footer');
      await expect(foot).toBeVisible();
      for (const filter of ['', 'no-such-item-zzz']) {
        if (filter) {
          await page.keyboard.press('/');
          await page.keyboard.type(filter);
        }
        const bottom = async (l: typeof host): Promise<number> => {
          const b = (await l.boundingBox())!;
          return b.y + b.height;
        };
        const want = await bottom(host);
        const msg = `${view} at ${height}px, filter "${filter}"`;
        await expect.poll(async () => Math.abs((await bottom(pane)) - want), msg).toBeLessThanOrEqual(1);
        await expect.poll(async () => Math.abs((await bottom(foot)) - want), msg).toBeLessThanOrEqual(1);
      }
    }
  }
});

test('Reviews keeps the brief beside the list down to a 900px window, under it below (#401)', async ({
  page,
}) => {
  await boot(page);
  const host = page.getByTestId('global-host');
  await page.getByTestId('rail-reviews').click();
  const pane = host.getByTestId('reviews-pane');
  await expect(host.getByTestId('review-brief')).toBeVisible();
  for (const [width, beside] of [
    [1440, true],
    [936, true],
    [860, false],
  ] as const) {
    await page.setViewportSize({ width, height: 900 });
    await expect(pane.locator('.row[data-key]').first()).toBeVisible();
    await expect
      .poll(async () => {
        const list = (await pane.locator('.list').boundingBox())!;
        const side = (await pane.locator('.side').boundingBox())!;
        return beside ? side.x >= list.x + list.width - 1 : side.y >= list.y + list.height - 1;
      }, `${width}px`)
      .toBe(true);
    // No row runs past the list: the title wraps, the state line ends in an ellipsis.
    const over = await pane
      .locator('.row[data-key]')
      .evaluateAll((rows) => rows.filter((r) => r.scrollWidth > r.clientWidth + 1).length);
    expect(over, `${width}px`).toBe(0);
  }
});
