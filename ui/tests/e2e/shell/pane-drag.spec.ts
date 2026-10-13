// Drag a pane by its header to reorganise the layout (ticket #341).
import { expect, test, type Page } from '@playwright/test';

import { boot, callsOf, unfold } from './helpers';

const panes = (page: Page) => page.locator('[data-testid="workspace"] [data-testid="pane"]');
const header = (page: Page, i: number) => panes(page).nth(i).getByTestId('pane-header');
const rows = (page: Page) => page.locator('[data-testid="rail-row"][data-project-id="shop"]');
const paneIds = (page: Page) =>
  page.evaluate(() =>
    [...document.querySelectorAll('[data-testid="workspace"] [data-testid="pane"]')].map((e) =>
      e.getAttribute('data-pane-id'),
    ),
  );
/** Pane ids of the saved layout's tabs (what a restart loads). */
async function savedTabs(page: Page): Promise<string[][]> {
  const saves = await callsOf(page, 'layout_save');
  // The save is debounced: before the first one there are no tabs yet, so polls retry.
  const layout = (saves.at(-1)?.args as { layout: { tabs: { root: unknown }[] } } | undefined)?.layout;
  if (!layout) return [];
  const ids = (n: unknown): string[] => {
    const node = n as { type: string; id: string; children?: unknown[] };
    return node.type === 'pane' ? [node.id] : (node.children ?? []).flatMap(ids);
  };
  return layout.tabs.map((t) => ids(t.root));
}

/** Drags pane `from` by its header onto pane `to` at the fraction (`fx`, `fy`) of its box. */
async function dragOver(page: Page, from: number, to: number, fx: number, fy: number): Promise<void> {
  const h = (await header(page, from).boundingBox())!;
  const box = (await panes(page).nth(to).boundingBox())!;
  const x = box.x + box.width * fx;
  const y = box.y + box.height * fy;
  await page.mouse.move(h.x + 40, h.y + h.height / 2);
  await page.mouse.down();
  await page.mouse.move(x, y, { steps: 6 });
  await page.mouse.move(x + 1, y, { steps: 2 });
}

async function dragOnto(page: Page, from: number, to: number, fx: number, fy: number): Promise<void> {
  await dragOver(page, from, to, fx, fy);
  await page.mouse.up();
}

test.describe('pane drag and drop', () => {
  test('dropping on an edge moves the pane to that side without re-attaching', async ({ page }) => {
    await boot(page);
    const [a, b] = await paneIds(page);
    const attaches = (await callsOf(page, 'session_attach')).length;
    await dragOnto(page, 1, 0, 0.5, 0.95);
    // b now sits below a: same x, stacked; it keeps the focus.
    await expect
      .poll(async () => (await panes(page).nth(0).boundingBox())!.height)
      .toBeLessThan((await page.getByTestId('panes').boundingBox())!.height * 0.7);
    const [boxA, boxB] = [
      (await page.locator(`[data-pane-id="${a}"]`).boundingBox())!,
      (await page.locator(`[data-pane-id="${b}"]`).boundingBox())!,
    ];
    expect(Math.round(boxA.x)).toBe(Math.round(boxB.x));
    expect(boxA.y).toBeLessThan(boxB.y);
    await expect(page.locator(`[data-pane-id="${b}"]`)).toHaveAttribute('data-focused', 'true');
    // Same sessions, same views: nothing spawned, killed or re-attached.
    expect((await callsOf(page, 'session_attach')).length).toBe(attaches);
    expect((await callsOf(page, 'session_spawn')).length).toBe(0);
    expect((await callsOf(page, 'session_kill')).length).toBe(0);
    await expect.poll(async () => (await savedTabs(page))[0]).toEqual([a, b]);
  });

  test('dropping on the centre swaps the panes', async ({ page }) => {
    await boot(page);
    const [a, b] = await paneIds(page);
    await dragOnto(page, 0, 1, 0.5, 0.5);
    await expect
      .poll(async () => (await page.locator(`[data-pane-id="${a}"]`).boundingBox())!.x)
      .toBeGreaterThan((await page.locator(`[data-pane-id="${b}"]`).boundingBox())!.x);
    await expect.poll(async () => (await savedTabs(page))[0]).toEqual([b, a]);
  });

  test('dropping on a rail row moves the pane to that tab, on the band to a new tab', async ({ page }) => {
    await boot(page);
    const [a, b] = await paneIds(page);
    await unfold(page, 'shop');
    await header(page, 0).dragTo(rows(page).nth(2));
    await expect(rows(page).nth(2)).toHaveAttribute('aria-current', 'page');
    await expect(page.locator(`[data-pane-id="${a}"]`)).toHaveAttribute('data-focused', 'true');
    await expect.poll(async () => (await savedTabs(page))[0]).toEqual([b]);

    // The project caption is band space outside the rows: a new tab.
    await expect(panes(page)).toHaveCount(2);
    await page
      .locator(`[data-pane-id="${a}"]`)
      .getByTestId('pane-header')
      .dragTo(page.locator('[data-testid="rail-project"][data-project-id="shop"]'));
    await expect(rows(page)).toHaveCount(4);
    await expect(rows(page).nth(3)).toHaveAttribute('aria-current', 'page');
    await expect.poll(() => paneIds(page)).toEqual([a]);
    await expect.poll(async () => (await savedTabs(page)).at(-1)).toEqual([a]);
    expect((await callsOf(page, 'session_kill')).length).toBe(0);
  });

  test('the drop indicator shows the side; Escape cancels the drag', async ({ page }) => {
    await boot(page);
    const before = await paneIds(page);
    await dragOver(page, 1, 0, 0.02, 0.5);
    await expect(page.getByTestId('pane-drop-indicator')).toHaveAttribute('data-zone', 'left');
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('pane-drop-layer')).toHaveCount(0);
    await page.mouse.up();
    expect(await paneIds(page)).toEqual(before);
    // The press may save its focus change (debounced), never a move.
    await page.waitForTimeout(700);
    if ((await callsOf(page, 'layout_save')).length > 0) expect((await savedTabs(page))[0]).toEqual(before);
  });
});
