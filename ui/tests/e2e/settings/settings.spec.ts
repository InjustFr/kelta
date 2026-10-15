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
    await page.getByTestId('applies-to').click();
    await page.getByRole('menuitem', { name: /^Shop(?! ›)/ }).click();
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

  test('Changed by you: a global and a project change, where each is set, narrowed by search, reset (#474)', async ({
    page,
  }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));
    await page.goto(`${HARNESS}?view=settings&section=terminal`);
    await expect(page.locator('[data-testid="settings-section"]')).toBeVisible();
    await page.evaluate(async () => {
      const { __ipc } = window as unknown as { __ipc: typeof Ipc };
      await __ipc.settingsSet({ layer: 'global', path: 'terminal.font_size', value: 15 });
      await __ipc.settingsSet({
        layer: 'project',
        project_id: 'shop',
        path: 'worktree.branch_template',
        value: 'feat/{key}',
      });
    });
    await page.getByRole('button', { name: /Changed by you/ }).click();
    const list = page.getByTestId('changed-settings');
    const font = list.locator('li', { hasText: 'terminal.font_size' });
    const branch = list.locator('li', { hasText: 'worktree.branch_template' });
    await expect(font).toContainText('All projects');
    await expect(branch).toContainText('Shop');

    await page.getByRole('searchbox', { name: 'Search settings' }).fill('font size');
    await expect(font).toBeVisible();
    await expect(branch).toHaveCount(0);

    await font.getByTestId('changed-reset').click();
    await expect(font).toHaveCount(0);
    const last = await page.evaluate(
      () =>
        (window as unknown as { __keltaMock: { calls: { cmd: string; args: unknown }[] } }).__keltaMock.calls
          .filter((c) => c.cmd === 'settings_reset')
          .at(-1)?.args,
    );
    expect(last).toMatchObject({ layer: 'global', path: 'terminal.font_size' });
    expect(errors).toEqual([]);
  });

  test('Applies to: overrides in All projects, inherited values and global-only rows in a project, repository trust (#475)', async ({
    page,
  }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));
    await page.goto(`${HARNESS}?view=settings&section=terminal`);
    await expect(page.locator('[data-testid="settings-section"]')).toBeVisible();
    // the mock shares one effective state: the global value goes last
    await page.evaluate(async () => {
      const { __ipc } = window as unknown as { __ipc: typeof Ipc };
      await __ipc.settingsSet({ layer: 'project', project_id: 'shop', path: 'terminal.font_size', value: 16 });
      await __ipc.settingsSet({ layer: 'global', path: 'terminal.font_size', value: 14 });
    });
    const font = page.locator('[data-testid="field"][data-path="terminal.font_size"]');
    await expect(font.getByTestId('override')).toHaveText('Shop uses 16 →');
    await font.getByTestId('override').click();
    await expect(page.getByTestId('applies-to')).toHaveText('Shop');
    await expect(page.getByTestId('layer-help')).toContainText('Changes here apply to Shop only.');
    await expect(font).toHaveAttribute('data-revealed', '');
    await expect(font.getByTestId('reset')).toHaveAccessibleName('Use the All projects value again');

    const line = page.locator('[data-testid="field"][data-path="terminal.line_height"]');
    await expect(line.getByRole('spinbutton')).toHaveValue('');
    await expect(line.getByRole('spinbutton')).toHaveAttribute('placeholder', '1.15');
    await expect(line).toContainText('Inherited from All projects');

    await page.locator('nav [data-section="general"]').click();
    const lang = page.locator('[data-testid="field"][data-path="app.language"]');
    await expect(lang.getByRole('combobox')).toHaveCount(0);
    await expect(lang).toContainText('The same for every project.');
    await lang.getByRole('button', { name: 'Change in All projects →' }).click();
    await expect(page.getByTestId('applies-to')).toHaveText('All projects');
    await expect(lang.getByRole('combobox')).toBeEnabled();

    await page.getByTestId('applies-to').click();
    await page.getByRole('menuitem', { name: /^Shop › api/ }).click();
    await expect(page.getByTestId('repo-untrusted')).toBeVisible();
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

  test('search: focus on open, Esc clears, ↓ and ↵ walk the hits (#469)', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));
    await page.goto(`${HARNESS}?view=settings&section=general`);
    const search = page.getByRole('searchbox', { name: 'Search settings' });
    await expect(search).toBeFocused();
    await page.keyboard.type('clipboard');
    const results = page.getByTestId('search-results');
    const hit = results.locator('[data-hit="key:terminal.copy_on_select"]');
    await expect(hit).toBeVisible();
    await expect(hit).toHaveClass(/active/);
    await page.keyboard.press('ArrowDown');
    await expect(hit).not.toHaveClass(/active/);
    await page.keyboard.press('ArrowUp');
    await expect(hit).toHaveClass(/active/);
    await page.keyboard.press('Enter');
    const sw = results.locator('[data-path="terminal.copy_on_select"]').getByRole('switch');
    await expect(sw).toBeFocused();
    await page.keyboard.press('Space');
    await expect(sw).toBeChecked();
    await search.focus();
    await page.keyboard.press('Escape');
    await expect(search).toHaveValue('');
    await expect(page.getByTestId('settings-pane')).toBeVisible();
    expect(errors).toEqual([]);
  });

  test('Terminal: grouped by task, Clipboard group, session host toggle, disk caps follow saving (#470)', async ({
    page,
  }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));
    await page.goto(`${HARNESS}?view=settings&section=terminal`);

    await expect(
      page.locator('[data-testid="settings-section"] > [data-testid="copy-group"] > header > h3'),
    ).toHaveText(['Text', 'Cursor', 'Clipboard', 'Keyboard', 'Scrollback & history', 'Shell', 'Sound']);
    const clipboard = page
      .locator('[data-testid="copy-group"]')
      .filter({ has: page.getByRole('heading', { name: 'Clipboard', exact: true }) });
    for (const p of ['copy_on_select', 'osc52', 'confirm_multiline_paste'])
      await expect(clipboard.locator(`[data-testid="field"][data-path="terminal.${p}"]`)).toBeVisible();
    await expect(clipboard.getByText('Let programs copy to the clipboard')).toBeVisible();
    await expect(page.locator('[data-testid="group"][data-path="terminal.scrollback"]')).toBeVisible();
    await expect(page.getByTestId('terminal-preview')).toBeVisible();

    await page.getByTestId('advanced').getByText('Advanced settings').click();
    for (const name of ['OSC 52', 'Session host', 'Shift enter'])
      await expect(page.getByText(name, { exact: true })).toHaveCount(0);
    await expect(page.locator('[data-testid="settings-section"] textarea')).toHaveCount(0);
    await expect(
      page
        .locator('[data-testid="field"][data-path="terminal.session_host"]')
        .getByRole('switch', { name: 'Keep sessions running after Kelta quits' }),
    ).toBeChecked();

    await page.getByRole('switch', { name: 'Save scrolled-off lines to disk' }).click();
    await expect(page.locator('[data-testid="field"][data-path="terminal.history_log_mb"]')).toHaveCount(0);
    expect(errors).toEqual([]);
  });

  test('Keys: prefix fields first, filter, press keys, inline clash, reset (#476)', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));
    await page.goto(`${HARNESS}?view=settings&section=keys`);

    const fields = page.locator('[data-testid="field"]');
    await expect(fields.nth(0)).toHaveAttribute('data-path', 'keys.prefix');
    await expect(fields.nth(1)).toHaveAttribute('data-path', 'keys.prefix_hint_ms');

    const rows = page.locator('[data-testid="bindings"] tr[data-action]');
    await page.getByTestId('keys-filter').fill('split');
    await expect(rows).toHaveCount(2);
    await expect(page.locator('tbody[data-area="panes"]')).toBeVisible();
    await page.getByTestId('keys-filter').fill('');

    await page.getByRole('button', { name: 'Press keys', exact: true }).click();
    await expect(page.getByTestId('press-keys')).toBeFocused();
    await page.keyboard.press('ControlOrMeta+Shift+d');
    await expect(rows).toHaveCount(1);
    await expect(rows.first()).toHaveAttribute('data-action', 'pane.split_down');
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('keys-filter')).toBeVisible();

    const detach = page.locator('tr[data-action="pane.detach"]');
    await detach.getByRole('button', { name: 'Record' }).click();
    await expect(page.getByTestId('recorder')).toBeFocused();
    await page.keyboard.press('ControlOrMeta+d');
    await expect(page.getByTestId('conflict')).toContainText('Split right');
    await page.getByRole('button', { name: 'Pick another' }).click();
    await expect(page.getByTestId('recorder')).toBeVisible();
    await page.keyboard.press('ControlOrMeta+d');
    await page.getByRole('button', { name: 'Use it here' }).click();

    const split = page.locator('tr[data-action="pane.split_right"]');
    await expect(split).toContainText('Unbound');
    await expect(split).toContainText('Changed');
    await expect(detach).toContainText('Changed');

    await page.getByRole('button', { name: /^Conflicts/ }).click();
    await expect(detach).toHaveCount(0);
    await expect(split).toHaveCount(0);
    await page.getByRole('button', { name: /^All/ }).click();

    await page.getByRole('button', { name: 'Reset Split right to default' }).click();
    await expect(split).not.toContainText('Unbound');
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
    // Esc in the search clears it first, then closes Settings.
    const box = page.getByRole('searchbox', { name: 'Search settings' });
    await box.fill('font');
    await page.keyboard.press('Escape');
    await expect(box).toHaveValue('');
    await expect(page.getByTestId('settings-pane')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('workspace')).toBeVisible();
  });
});
