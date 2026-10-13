// Settings › Health (#411): one row per problem with a button that fixes it or opens its setting.
import { expect, test } from '@playwright/test';

import { boot, callsOf, dispatch } from '../shell/helpers';

test.use({ colorScheme: 'dark' });

test('Health lists what is broken and each button fixes it or opens its setting', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (err) => errors.push(err.message));
  await boot(page);
  await page.evaluate(async () => {
    const mock = window.__keltaMock!;
    // an account rejected, an update waiting, a restart-only key changed, a theme the floor cannot fix
    mock.emit({ type: 'account.status', account_id: 'jira-acme', status: 'needs_auth', detail: '401' });
    mock.state.update = { ...mock.state.update, state: { type: 'ready', version: '9.9.9' } };
    const value = mock.state.settings.value as { app: Record<string, unknown>; themes?: unknown };
    value.app.dark_theme = 'murky';
    value.themes = {
      murky: { name: 'Murky', base: 'dark', ui: { well: '#777777', fg: '#7a7a7a' }, terminal: {} },
    };
    mock.emit({
      type: 'settings.changed',
      layers: ['global'],
      paths: ['themes'],
      requires_restart: ['window.decorations'],
    });
    // a source whose tickets fail to load
    mock.failNext('tracker_list', { code: 'network', message: 'connection refused' });
    await window.__kelta!.stores.tickets.load({ kind: 'all' }, null, false, null, 'board').catch(() => {});
    // the default load, across every source
    mock.failNext('tracker_list', { code: 'network', message: 'connection refused' });
    await window.__kelta!.stores.tickets.load({ kind: 'all' }, null, false, null, null).catch(() => {});
  });
  await dispatch(page, 'settings.open', { section: 'health' });

  const nav = page.getByRole('navigation', { name: 'Settings sections' });
  const row = (id: string) => page.locator(`[data-testid="health-problem"][data-problem="${id}"]`);
  const ids = [
    'claude-hooks',
    'claude-mcp',
    'account:jira-acme',
    'source:board',
    'source:all',
    'restart',
    'update',
    'theme',
  ];
  for (const id of ids) await expect(row(id)).toBeVisible();
  await expect(nav.getByTestId('health-count')).toHaveText(String(ids.length));

  await row('claude-hooks').getByRole('button', { name: 'Install hooks' }).click();
  await expect(row('claude-hooks')).toHaveCount(0);
  await row('claude-mcp').getByRole('button', { name: 'Install MCP' }).click();
  await expect(row('claude-mcp')).toHaveCount(0);
  await expect(nav.getByTestId('health-count')).toHaveText(String(ids.length - 2));

  await row('restart').getByRole('button', { name: 'Restart' }).click();
  await expect.poll(async () => (await callsOf(page, 'app_restart')).length).toBe(1);
  await row('update').getByRole('button', { name: 'Restart' }).click();
  await expect.poll(async () => (await callsOf(page, 'app_restart')).length).toBe(2);

  const opens: [string, string, string][] = [
    ['account:jira-acme', 'Reconnect account', 'accounts'],
    ['source:board', 'Open source', 'sources'],
    ['source:all', 'Open source', 'sources'],
    ['theme', 'Open theme', 'general'],
  ];
  for (const [id, label, section] of opens) {
    await dispatch(page, 'settings.open', { section: 'health' });
    await row(id).getByRole('button', { name: label }).click();
    await expect(nav.locator(`[data-section="${section}"]`)).toHaveAttribute('aria-current', 'page');
  }
  expect(errors).toEqual([]);
});
