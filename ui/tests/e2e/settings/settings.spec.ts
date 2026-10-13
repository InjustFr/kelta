import { expect, test } from '@playwright/test';

import type * as Ipc from '$lib/ipc/commands';

import { boot, dispatch } from '../shell/helpers';

const HARNESS = '/tests/e2e/settings/harness.html';

test.describe('settings pane', () => {
  test('a key set at the project layer shows the Project badge', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));

    await page.goto(`${HARNESS}?view=settings&section=terminal`);
    const field = page.locator('[data-testid="field"][data-path="terminal.font_size"]');
    await expect(field).toBeVisible();
    await expect(field.locator('[data-testid="source-badge"]')).toHaveAttribute('data-source', 'default');

    // switch to the Project layer
    await page.getByRole('button', { name: 'Project', exact: true }).click();
    await expect(page.locator('[data-testid="settings-section"]')).toHaveAttribute('data-layer', 'project');

    const input = field.getByRole('spinbutton');
    await input.fill('15');
    await input.press('Tab');
    await expect(field.locator('[data-testid="source-badge"]')).toHaveAttribute('data-source', 'project');
    await expect(field.locator('[data-testid="source-badge"]')).toContainText('Project');
    await expect(input).toHaveValue('15');

    // reset at this layer brings the default back
    await field.getByTestId('reset').click();
    await expect(field.locator('[data-testid="source-badge"]')).toHaveAttribute('data-source', 'default');
    expect(errors).toEqual([]);
  });

  test('a source without routes, projects or columns opens Sources and takes a route (#281)', async ({
    page,
  }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));
    await page.goto(`${HARNESS}?view=settings&section=terminal`);
    await expect(page.locator('[data-testid="settings-section"]')).toBeVisible();
    await page.evaluate(async () => {
      const { __ipc } = window as unknown as { __ipc: typeof Ipc };
      const view = { current_iteration: false, id: 'mine', label: 'My tickets', repo: 'InjustFr/kelta' };
      const value = [
        { id: 'my-tickets', label: 'My tickets', tracker: { account: 'injustfr', views: [view] } },
      ];
      await __ipc.settingsSet({ layer: 'global', path: 'sources', value });
    });
    await page.getByRole('navigation', { name: 'Settings sections' }).getByText('Sources').click();
    const card = page.locator('[data-source-card="my-tickets"]');
    await expect(card).toBeVisible();
    await card.getByRole('button', { name: 'Shop' }).click();
    await card.getByRole('button', { name: 'Add route' }).click();
    await expect(card.getByTestId('source-routes')).toBeVisible();
    expect(errors).toEqual([]);
  });

  test("Settings → Claude installs and removes Kelta's MCP in Claude Code", async ({ page }) => {
    await page.goto(`${HARNESS}?view=settings&section=claude`);
    const mcp = page.getByTestId('claude-mcp');
    await expect(mcp).toContainText('Install Kelta');
    await mcp.getByRole('button', { name: 'Install' }).click();
    await expect(mcp).toContainText('are installed in Claude Code');
    await mcp.getByRole('button', { name: 'Remove' }).click();
    await expect(mcp.getByRole('button', { name: 'Install' })).toBeVisible();
    // #277: the mode of sessions whose profile forces none
    await expect(page.getByTestId('claude-permission-mode')).toContainText('last used: auto');
  });

  test('grouped nav, advanced keys collapsed, search by label and path, TOML-only link (#282)', async ({
    page,
  }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));
    await page.goto(`${HARNESS}?view=settings&section=terminal`);
    const nav = page.getByRole('navigation', { name: 'Settings sections' });
    await expect(nav.getByRole('heading')).toHaveText(['Basics', 'Work', 'Integrations', 'Advanced']);

    // a rarely changed key sits in the closed Advanced block
    const advanced = page.getByTestId('advanced');
    await expect(advanced.locator('[data-testid="field"][data-path="terminal.renderer"]')).toBeHidden();
    await advanced.getByText('Advanced settings').click();
    await expect(advanced.locator('[data-testid="field"][data-path="terminal.renderer"]')).toBeVisible();

    const search = page.getByRole('searchbox', { name: 'Search settings' });
    await search.fill('font size');
    await expect(
      page.locator('[data-testid="search-results"] [data-testid="field"][data-path="terminal.font_size"]'),
    ).toBeVisible();
    await search.fill('reopen sessions');
    await expect(
      page.locator('[data-testid="search-results"] [data-testid="field"][data-path="app.restore_mode"]'),
    ).toBeVisible();
    await search.fill('polling.focused_secs');
    await expect(page.locator('[data-testid="search-results"] [data-testid="field"]')).toHaveCount(1);

    await search.fill('');
    await nav.getByText('Triggers').click();
    await page.getByTestId('toml-only').getByRole('button', { name: 'Edit TOML' }).click();
    await expect(page.getByTestId('edit-toml-toggle')).toHaveText('Back to form');
    expect(errors).toEqual([]);
  });
});

test.describe('settings over the window (#410)', () => {
  test('⌘, opens Settings without a tab; Esc returns to the tab and pane', async ({ page }) => {
    await boot(page, 'macos');
    const where = () =>
      page.evaluate(() => {
        const l = window.__kelta!.stores.layout.get('shop')!;
        const tab = l.tabs.find((t) => t.id === l.active_tab);
        return { tabs: l.tabs.map((t) => t.id), active: l.active_tab, focused: tab?.focused_pane };
      });
    const before = await where();
    await page.keyboard.press('Meta+Comma');
    await expect(page.getByTestId('settings-pane')).toBeVisible();
    await expect(page.getByTestId('workspace')).toHaveCount(0);
    await expect(page.getByTestId('rail')).toBeVisible();
    await expect(page.getByTestId('rail-settings')).toHaveAttribute('aria-current', 'page');
    expect(await where()).toEqual(before);

    await page.keyboard.press('Escape');
    await expect(page.getByTestId('settings-pane')).toHaveCount(0);
    await expect(page.getByTestId('workspace')).toBeVisible();
    expect(await where()).toEqual(before);

    // ⌘, again closes it too.
    await page.keyboard.press('Meta+Comma');
    await expect(page.getByTestId('settings-pane')).toBeVisible();
    await page.keyboard.press('Meta+Comma');
    await expect(page.getByTestId('settings-pane')).toHaveCount(0);
  });

  test('deep links land on their section from the palette and from a banner action', async ({ page }) => {
    await boot(page);
    await page.keyboard.press('Control+Shift+K');
    await page.getByTestId('palette-input').fill('settings keys');
    await page.keyboard.press('Enter');
    const current = page.locator('[data-testid="settings-pane"] nav [aria-current="page"]');
    await expect(current).toHaveAttribute('data-section', 'keys');
    // A banner's action dispatches the same deep link, with Settings already showing.
    await dispatch(page, 'settings.open', { section: 'projects' });
    await expect(current).toHaveAttribute('data-section', 'projects');
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('workspace')).toBeVisible();
  });
});
