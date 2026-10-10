// Scaffold smoke test on the IPC mock (VITE_IPC=mock). Must keep passing after every lane merge:
// it only relies on the frozen contracts (main.ts debug handles, stores, registries), not on the
// shell's markup.
import { expect, test } from '@playwright/test';

test.describe('scaffold', () => {
  test('boots on the mock IPC, loads stores and every lazy view', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));
    page.on('console', (msg) => {
      if (msg.type() === 'error') errors.push(msg.text());
    });

    await page.goto('/');
    await expect(page.locator('#app')).not.toBeEmpty();
    await page.waitForFunction(() => window.__kelta?.stores.projects.loaded === true);

    const state = await page.evaluate(async () => {
      const k = window.__kelta!;
      const result = await k.stores.bootstrap();
      return {
        failed: result.failed,
        projects: k.stores.projects.list.length,
        active: k.stores.projects.activeId,
        sessions: k.stores.sessions.all.length,
        layoutTabs: k.stores.layout.get('shop')?.tabs.length ?? 0,
        work: k.stores.work.all.length,
        theme: k.stores.settings.value()?.app.theme ?? null,
        mock: window.__keltaMock !== undefined,
      };
    });
    expect(state).toEqual({
      failed: [],
      projects: 4,
      active: 'shop',
      sessions: 11,
      layoutTabs: 3,
      work: 9,
      theme: 'system',
      mock: true,
    });

    // Every registry entry is a separately loadable chunk.
    const loaded = await page.evaluate(async () => {
      const r = window.__kelta!.registry;
      const loaders = [
        ...Object.values(r.paneRegistry),
        ...Object.values(r.sheetRegistry),
        ...Object.values(r.tabHeaderRegistry),
        ...r.settingsSections.map((s) => s.load),
      ];
      const mods = await Promise.all(loaders.map((l) => l()));
      return mods.filter((m) => typeof m.default === 'function').length;
    });
    expect(loaded).toBe(12 + 17 + 1 + 17);

    // UiEvents from the backend reach the stores.
    await page.evaluate(() => {
      window.__keltaMock!.emit({
        type: 'toast',
        toast: { level: 'info', text: 'hello from mock', action: null },
      });
    });
    await page.waitForFunction(() =>
      window.__kelta!.stores.toasts.list.some((t) => t.toast.text === 'hello from mock'),
    );

    expect(errors).toEqual([]);
  });

  test('sprite and theme tokens are in place', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('#kelta-sprite symbol#i-x')).toHaveCount(1);
    const bg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    expect(bg).not.toBe('');
  });
});
