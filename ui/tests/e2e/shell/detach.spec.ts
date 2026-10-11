// Move a pane or a tab to its own window (ticket #342). The mock has no native windows: these specs
// check the layout side (detached flag, main window placeholder, window_detach) and the detached page.
import { expect, test, type Page } from '@playwright/test';

import { boot, callsOf, waitForAttached } from './helpers';

const panes = (page: Page) => page.locator('[data-testid="workspace"] [data-testid="pane"]');

interface SavedTab {
  id: string;
  detached?: boolean;
}

async function savedTabs(page: Page): Promise<SavedTab[]> {
  const saves = await callsOf(page, 'layout_save');
  const layout = (saves.at(-1)?.args as { layout: { project_id: string; tabs: SavedTab[] } } | undefined)
    ?.layout;
  return layout?.project_id === 'shop' ? layout.tabs : [];
}

test.describe('detached windows', () => {
  test('a pane moves to a new window and comes back', async ({ page }) => {
    await boot(page);
    const moved = await panes(page).first().getAttribute('data-pane-id');
    await page.getByRole('button', { name: 'Pane menu' }).first().click();
    await page.getByRole('menuitem', { name: 'Move to new window' }).click();

    // The pane left the main window in a tab of its own, saved before the window opens.
    await expect(panes(page)).toHaveCount(1);
    await expect.poll(async () => (await callsOf(page, 'window_detach')).length).toBe(1);
    const [call] = await callsOf(page, 'window_detach');
    const tabId = (call!.args as { tab_id: string }).tab_id;
    expect(call!.args).toMatchObject({ project_id: 'shop' });
    expect((await savedTabs(page)).find((t) => t.id === tabId)?.detached).toBe(true);

    // Its rail row shows the placeholder in the main window; "Move back here" brings it back.
    await page.locator(`[data-testid="rail-row"][data-tab-id="${tabId}"]`).click();
    await expect(page.getByText('This tab is in its own window')).toBeVisible();
    await page.getByRole('button', { name: 'Move back here' }).click();
    await expect(page.locator(`[data-pane-id="${moved}"]`)).toBeVisible();
    await expect.poll(async () => (await savedTabs(page)).find((t) => t.id === tabId)?.detached).toBe(false);
  });

  test('a pane dragged out of the window moves to a new window; a drop inside does not', async ({ page }) => {
    await boot(page);
    const header = panes(page).first().getByTestId('pane-header');
    await header.dispatchEvent('dragstart');
    await header.dispatchEvent('dragend', { clientX: 200, clientY: 200 });
    await header.dispatchEvent('dragstart');
    await header.dispatchEvent('dragend', { clientX: -20, clientY: 200 });
    await expect(panes(page)).toHaveCount(1);
    await expect.poll(async () => (await callsOf(page, 'window_detach')).length).toBe(1);
  });

  test('a detached window shows its tab alone, attached to the same sessions', async ({ page }) => {
    await page.addInitScript(() => {
      window.__KELTA_DETACHED__ = { project_id: 'shop', tab_id: 'shop-t1' };
    });
    await page.goto('/');
    await page.waitForFunction(() => window.__kelta?.stores.projects.loaded === true);
    await expect(page.getByTestId('workspace')).toHaveAttribute('data-project-id', 'shop');
    await waitForAttached(page, 2);
    await expect(page.getByTestId('rail-row')).toHaveCount(0);
    await page.getByRole('button', { name: 'Pane menu' }).first().click();
    await expect(page.getByRole('menuitem', { name: 'Move back to main window' })).toBeVisible();
  });
});
