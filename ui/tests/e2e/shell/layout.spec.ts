// Split, resize, zoom and close on the mock layout (ARCH §5.1).
import { expect, test, type Page } from '@playwright/test';

import { boot, callLog, callsOf, dispatch, waitForAttached } from './helpers';

const panes = (page: Page) => page.locator('[data-testid="workspace"] [data-testid="pane"]');

async function paneWidths(page: Page): Promise<number[]> {
  return page.evaluate(() =>
    [...document.querySelectorAll('[data-testid="pane"]')].map((e) => e.getBoundingClientRect().width),
  );
}

test.describe('layout', () => {
  test('split right spawns a shell and adds a pane; the new pane is focused', async ({ page }) => {
    await boot(page);
    await expect(panes(page)).toHaveCount(2);
    await dispatch(page, 'pane.split_right');
    await expect(panes(page)).toHaveCount(3);
    await waitForAttached(page, 3);
    expect((await callsOf(page, 'session_spawn')).length).toBe(1);
    const spawn = (await callsOf(page, 'session_spawn'))[0]!.args as {
      req: { kind: { type: string }; cwd: string };
    };
    expect(spawn.req.kind).toEqual({ type: 'shell' });
    // The cwd follows the focused session.
    expect(spawn.req.cwd).toBe('/Users/ada/code/shop-api.worktrees/SHOP-142');
    // The new pane sits right of the focused one and takes the focus.
    await expect(panes(page).nth(1)).toHaveAttribute('data-focused', 'true');
    await expect(panes(page).nth(0)).toHaveAttribute('data-focused', 'false');
    // Existing views were not re-attached by the structural change (flat, keyed DOM).
    expect((await callsOf(page, 'session_attach')).length).toBe(3);
    // The layout is persisted (debounced) with the optimistic rev.
    await expect.poll(async () => (await callsOf(page, 'layout_save')).length).toBeGreaterThan(0);
  });

  test('split down stacks panes', async ({ page }) => {
    await boot(page);
    await dispatch(page, 'pane.split_down');
    await expect(panes(page)).toHaveCount(3);
    const boxes = await page.evaluate(() =>
      [...document.querySelectorAll('[data-testid="pane"]')].map((e) => {
        const r = e.getBoundingClientRect();
        return { x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.width), h: Math.round(r.height) };
      }),
    );
    // The first pane was split vertically: same x, stacked.
    expect(boxes[0]!.x).toBe(boxes[1]!.x);
    expect(boxes[0]!.y).toBeLessThan(boxes[1]!.y);
  });

  test('dragging a gutter resizes panes; ratios stay above 5 %', async ({ page }) => {
    await boot(page);
    const [w0, w1] = await paneWidths(page);
    const gutter = page.getByTestId('gutter').first();
    const box = (await gutter.boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 + 200, box.y + box.height / 2, { steps: 5 });
    await page.mouse.up();
    await expect.poll(async () => (await paneWidths(page))[0]).toBeGreaterThan(w0! + 150);
    const [a, b] = await paneWidths(page);
    expect(a! + b!).toBeCloseTo(w0! + w1!, 0);

    // Dragging far beyond the edge clamps at 5 %.
    const g2 = (await gutter.boundingBox())!;
    await page.mouse.move(g2.x + g2.width / 2, g2.y + g2.height / 2);
    await page.mouse.down();
    await page.mouse.move(g2.x + 5000, g2.y + g2.height / 2, { steps: 4 });
    await page.mouse.up();
    const ratio = await page.evaluate(() => {
      const layout = window.__kelta!.stores.layout.get('shop')!;
      const root = layout.tabs[0]!.root;
      return root.type === 'split' ? root.ratios : [];
    });
    expect(ratio[1]).toBeGreaterThanOrEqual(0.05 - 1e-6);
    expect(ratio[0]! + ratio[1]!).toBeCloseTo(1, 5);
    // Resizing is saved (debounced one-shot), not per pointer move.
    await expect.poll(async () => (await callsOf(page, 'layout_save')).length).toBeGreaterThan(0);
    expect((await callsOf(page, 'layout_save')).length).toBeLessThanOrEqual(2);
  });

  test('keyboard resize on the focused gutter', async ({ page }) => {
    await boot(page);
    const [w0] = await paneWidths(page);
    await page.getByTestId('gutter').first().focus();
    await page.keyboard.press('ArrowRight');
    await page.keyboard.press('ArrowRight');
    await expect.poll(async () => (await paneWidths(page))[0]).toBeGreaterThan(w0!);
  });

  test('zoom hides siblings without detaching them; close keeps sessions running', async ({ page }) => {
    await boot(page);
    await dispatch(page, 'pane.zoom');
    await expect(page.locator('[data-testid="pane"]:visible')).toHaveCount(1);
    await expect(page.getByTestId('gutter')).toHaveCount(0);
    const full = await page.evaluate(() => {
      const pane = document.querySelector('[data-testid="pane"]:not(.hidden)')!.getBoundingClientRect();
      const area = document.querySelector('[data-testid="panes"]')!.getBoundingClientRect();
      return { pane: pane.width, area: area.width };
    });
    expect(full.pane).toBeGreaterThan(full.area - 8);
    await dispatch(page, 'pane.zoom');
    await expect(page.locator('[data-testid="pane"]:visible')).toHaveCount(2);

    await dispatch(page, 'pane.close');
    await expect(panes(page)).toHaveCount(1);
    expect((await callLog(page)).filter((c) => c.cmd === 'session_kill')).toEqual([]);
    // The closed pane's session is still a background session of the project.
    const alive = await page.evaluate(
      () => window.__kelta!.stores.sessions.forProject('shop').filter((s) => s.lifecycle === 'live').length,
    );
    expect(alive).toBe(4);
    // The last pane of the tab closes the tab.
    await dispatch(page, 'pane.close');
    await expect(page.getByTestId('tab')).toHaveCount(2);
  });

  test('closed sessions stay in the palette and reopen in a new tab', async ({ page }) => {
    await boot(page);
    await page.getByTestId('pane-close').first().click();
    await expect(panes(page)).toHaveCount(1);
    await page.keyboard.press('Control+Shift+K');
    await page.getByTestId('palette-input').fill('shop claude');
    await expect(page.getByTestId('palette-item').first()).toContainText('claude');
    await page.keyboard.press('Enter');
    await expect(page.getByTestId('tab')).toHaveCount(4);
    await waitForAttached(page, 1);
  });

  test('focus moves with the pane focus actions', async ({ page }) => {
    await boot(page);
    await expect(panes(page).nth(0)).toHaveAttribute('data-focused', 'true');
    await dispatch(page, 'pane.focus_right');
    await expect(panes(page).nth(1)).toHaveAttribute('data-focused', 'true');
    await dispatch(page, 'pane.focus_left');
    await expect(panes(page).nth(0)).toHaveAttribute('data-focused', 'true');
  });

  test('tabs: next/prev, middle click closes, live sessions ask first', async ({ page }) => {
    await boot(page);
    const tabs = page.getByTestId('tab');
    await expect(tabs).toHaveCount(3);
    await dispatch(page, 'tab.next');
    await expect(tabs.nth(1)).toHaveAttribute('aria-selected', 'true');
    await dispatch(page, 'tab.prev');
    await expect(tabs.nth(0)).toHaveAttribute('aria-selected', 'true');
    // The Board tab has no process: it closes without a question.
    await tabs.nth(2).click({ button: 'middle' });
    await expect(tabs).toHaveCount(2);
    // The Shell tab has two live sessions: confirmation first.
    await tabs.nth(1).click({ button: 'middle' });
    await expect(page.getByTestId('confirm-dialog')).toBeVisible();
    await page.getByTestId('confirm-keep').click();
    await expect(tabs).toHaveCount(1);
    expect((await callsOf(page, 'session_kill')).length).toBe(0);
  });
});
