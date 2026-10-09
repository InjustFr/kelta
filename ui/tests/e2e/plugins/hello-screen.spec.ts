// L8 acceptance on the mock IPC: installing examples/plugins/hello-screen shows its permissions,
// opens its screen in a sandboxed iframe, and a call outside its grants is denied.
import { expect, test } from '@playwright/test';

import type * as HarnessModule from './harness';

type Harness = typeof HarnessModule;

test('install hello-screen, open its screen, deny an ungranted call', async ({ page }) => {
  await page.goto('/');
  await page.waitForFunction(() => window.__kelta?.stores.projects.loaded === true);

  await page.evaluate(async () => {
    const h = (await import('/tests/e2e/plugins/harness.ts' as string)) as Harness;
    h.useHelloScreen();
    const close = await h.mountView(window.__kelta!.registry.sheetRegistry.plugin_install, {
      onclose: () => close(),
    });
  });

  await page.getByLabel('Source').fill('../examples/plugins/hello-screen');
  await page.getByRole('button', { name: 'Inspect' }).click();
  const perms = page.getByTestId('plugin-permissions');
  await expect(perms).toContainText('See your projects');
  await expect(perms).toContainText('See your terminal sessions');
  await expect(perms).toContainText('Show desktop notifications');
  await expect(page.getByText('SHA-256')).toBeVisible();
  await page.getByRole('button', { name: 'Install' }).click();
  await expect(page.getByRole('dialog', { name: 'Install plugin' })).toHaveCount(0);

  await page.evaluate(async () => {
    const h = (await import('/tests/e2e/plugins/harness.ts' as string)) as Harness;
    await h.mountView(window.__kelta!.registry.paneRegistry.plugin_screen, {
      projectId: 'shop',
      tabId: 'tab-x',
      paneId: 'pane-x',
      content: {
        kind: 'plugin_screen',
        plugin_id: 'hello-screen',
        screen_id: 'hello',
        instance_id: 'old',
        params: null,
      },
      visible: true,
      focused: true,
    });
  });
  const frame = page.locator('iframe[src^="kelta-plugin://hello-screen/"]');
  await expect(frame).toHaveCount(1);
  await expect(frame).toHaveAttribute('sandbox', 'allow-scripts allow-forms');

  const denied = await page.evaluate(async () => {
    const h = (await import('/tests/e2e/plugins/harness.ts' as string)) as Harness;
    const { pluginCall } = await import('/src/lib/ipc/commands.ts' as string);
    const instance_id = h.opened[h.opened.length - 1].instance_id;
    const ok = await pluginCall({ instance_id, method: 'projects.list', params: {} });
    try {
      await pluginCall({ instance_id, method: 'tickets.list', params: {} });
      return { ok: Array.isArray(ok), code: null };
    } catch (e) {
      return { ok: Array.isArray(ok), code: (e as { code: string }).code };
    }
  });
  expect(denied).toEqual({ ok: true, code: 'permission_denied' });
});
