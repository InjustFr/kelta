// Project switching keeps sessions alive and re-uses pooled terminal views (ARCH §9.1, SPEC §1).
import { expect, test } from '@playwright/test';

import { activeProject, boot, callLog, callsOf, SESSIONS, waitForAttached } from './helpers';

test.describe('project switching', () => {
  test('never kills a session and re-attaches pooled views without a snapshot', async ({ page }) => {
    await boot(page);
    const shopSessions = [SESSIONS.shopClaude, SESSIONS.shopNvim];
    const attachesOf = async (id: string): Promise<number> =>
      (await callsOf(page, 'session_attach')).filter((c) => c.args?.id === id).length;
    for (const id of shopSessions) expect(await attachesOf(id)).toBe(1);

    // shop → billing (three panes: Claude, nvim, an exited shell)
    await page
      .getByTestId('rail-project')
      .filter({ has: page.locator('.glyph', { hasText: 'B' }) })
      .click();
    await expect.poll(() => activeProject(page)).toBe('billing');
    await expect(page.locator('[data-testid="workspace"][data-project-id="billing"]')).toBeVisible();
    await waitForAttached(page, 3);

    // billing → shop: the two shop views come back from the pool.
    await page
      .getByTestId('rail-project')
      .filter({ has: page.locator('.glyph', { hasText: 'S' }) })
      .click();
    await expect.poll(() => activeProject(page)).toBe('shop');
    await waitForAttached(page, 2);

    expect((await callLog(page)).filter((c) => c.cmd === 'session_kill')).toEqual([]);
    expect((await callLog(page)).filter((c) => c.cmd === 'session_detach')).toEqual([]);
    for (const id of shopSessions) expect(await attachesOf(id)).toBe(1);
    // The views are visible again and still hold their content (no snapshot repaint needed).
    await expect(page.locator('[data-project-id="shop"] .xterm')).toHaveCount(2);
  });

  test('a warm project switch takes at most 50 ms', async ({ page }) => {
    await boot(page);
    // Visit every project once so layouts and views are warm (mount lazy chunks, create views).
    const goto = async (n: number): Promise<void> => {
      await page.evaluate((i) => window.__kelta!.actions.dispatch(`project.goto.${i}`), n);
    };
    await goto(2); // billing (rail: shop, billing, kelta-tools, home)
    await expect(page.locator('[data-testid="workspace"][data-project-id="billing"] .xterm')).toHaveCount(3);
    await goto(1);
    await expect(page.locator('[data-testid="workspace"][data-project-id="shop"] .xterm')).toHaveCount(2);

    const timings = await page.evaluate(async () => {
      const wait = (selector: string, count: number): Promise<number> =>
        new Promise((resolve) => {
          const t0 = performance.now();
          const check = (): boolean => document.querySelectorAll(selector).length >= count;
          const obs = new MutationObserver(() => {
            if (check()) {
              obs.disconnect();
              resolve(performance.now() - t0);
            }
          });
          obs.observe(document.body, { childList: true, subtree: true });
          // The switch is triggered after the observer exists.
          window.__kelta!.actions.dispatch(window.__switchTo!);
        });
      const out: number[] = [];
      for (let i = 0; i < 4; i += 1) {
        window.__switchTo = i % 2 === 0 ? 'project.goto.2' : 'project.goto.1';
        const sel =
          i % 2 === 0
            ? '[data-testid="workspace"][data-project-id="billing"] .xterm'
            : '[data-testid="workspace"][data-project-id="shop"] .xterm';
        out.push(await wait(sel, i % 2 === 0 ? 3 : 2));
        // let the previous workspace settle
        await new Promise((r) => setTimeout(r, 50));
      }
      return out;
    });
    test
      .info()
      .annotations.push({ type: 'timings-ms', description: timings.map((t) => t.toFixed(1)).join(', ') });
    for (const ms of timings) expect(ms).toBeLessThanOrEqual(50);

    // The in-app mark (ARCH §13) agrees.
    const measures = await page.evaluate(() =>
      performance.getEntriesByName('kelta.project_switch').map((e) => e.duration),
    );
    expect(measures.length).toBeGreaterThan(0);
    for (const ms of measures.slice(-4)) expect(ms).toBeLessThanOrEqual(50);
  });

  test('rail: attention dots, inbox badge and context menu reorder', async ({ page }) => {
    await boot(page);
    const billing = page.locator('[data-testid="rail-project"][data-project-id="billing"]');
    await expect(billing).toHaveAttribute('data-attention', 'needs_input');
    await expect(page.locator('[data-testid="rail-project"][data-project-id="kelta-tools"]')).toHaveAttribute(
      'data-attention',
      'done',
    );
    await expect(page.getByTestId('inbox-badge')).toBeVisible();

    // Backend events move the dot.
    await page.evaluate(() =>
      window.__keltaMock!.emit({
        type: 'attention.changed',
        project_id: 'billing',
        level: 'none',
        needs_input_count: 0,
        total_needs_input: 0,
      }),
    );
    await expect(billing).toHaveAttribute('data-attention', 'none');

    // Context menu: Move up reorders the rail without touching sessions.
    await billing.click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Move up' }).click();
    await expect
      .poll(() =>
        page.evaluate(() =>
          [...document.querySelectorAll('[data-testid="rail-project"]')].map((e) =>
            e.getAttribute('data-project-id'),
          ),
        ),
      )
      .toEqual(['billing', 'shop', 'kelta-tools', 'home']);
    expect((await callsOf(page, 'project_reorder')).length).toBe(1);
    expect((await callsOf(page, 'session_kill')).length).toBe(0);
  });

  test('dragging a project on the rail reorders it', async ({ page }) => {
    await boot(page);
    const item = (id: string) => page.locator(`[data-testid="rail-project"][data-project-id="${id}"]`);
    await item('kelta-tools').dragTo(item('shop'));
    await expect
      .poll(() =>
        page.evaluate(() =>
          [...document.querySelectorAll('[data-testid="rail-project"]')].map((e) =>
            e.getAttribute('data-project-id'),
          ),
        ),
      )
      .toEqual(['kelta-tools', 'shop', 'billing', 'home']);
    const order = (await callsOf(page, 'project_reorder'))[0]!.args as { ids: string[] };
    expect(order.ids.indexOf('kelta-tools')).toBeLessThan(order.ids.indexOf('shop'));
    expect((await callsOf(page, 'session_kill')).length).toBe(0);
  });

  test('a backend ui.open request opens the pane and brings its project to the front', async ({ page }) => {
    await boot(page);
    await page.evaluate(() =>
      window.__keltaMock!.emit({
        type: 'ui.open',
        project_id: 'billing',
        request: {
          content: { kind: 'diagnostics' },
          placement: 'new_tab',
          focus: true,
          tab_title: 'Diagnostics',
          work_item_id: null,
        },
      }),
    );
    await expect.poll(() => activeProject(page)).toBe('billing');
    await expect(page.getByTestId('tab').filter({ hasText: 'Diagnostics' })).toHaveCount(1);
    expect((await callsOf(page, 'session_kill')).length).toBe(0);
  });

  test('closing a project keeps its sessions running unless stop is chosen', async ({ page }) => {
    await boot(page);
    const tools = page.locator('[data-testid="rail-project"][data-project-id="kelta-tools"]');
    await tools.click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Close project', exact: true }).click();
    await expect(tools).toHaveCount(0);
    expect((await callsOf(page, 'project_close'))[0]?.args).toMatchObject({
      id: 'kelta-tools',
      kill_sessions: false,
    });
    expect((await callsOf(page, 'session_kill')).length).toBe(0);
  });
});

declare global {
  interface Window {
    __switchTo?: string;
  }
}
