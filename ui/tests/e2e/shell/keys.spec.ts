// Key routing end to end: Kelta chords vs. terminal passthrough, the app prefix and the
// disabled webview shortcuts.
import { expect, test, type Page } from '@playwright/test';

import { boot, callsOf, dispatch, SESSIONS, unfold } from './helpers';

/** Bytes written to a session (decoded) by `session_write`. */
async function written(page: Page, id: string): Promise<string[]> {
  const calls = await callsOf(page, 'session_write');
  return calls
    .filter((c) => c.args?.id === id)
    .map((c) => {
      const data = c.args?.data as Record<string, number> | Uint8Array | number[];
      const arr = Array.isArray(data) ? data : Object.values(data as Record<string, number>);
      return String.fromCharCode(...arr);
    });
}

test.describe('keys', () => {
  test('plain Ctrl+letter, Alt chords, Shift+Tab, Ctrl+Space and Ctrl+\\ reach the terminal', async ({
    page,
  }) => {
    await boot(page);
    await page.locator('[data-testid="pane"][data-focused="true"] .xterm-helper-textarea').focus();
    for (const chord of [
      'Control+a',
      'Control+r',
      'Control+f',
      'Control+w',
      'Control+k',
      'Control+d',
      'Alt+b',
      'Shift+Tab',
      'Control+Space',
      'Control+\\',
    ]) {
      await page.keyboard.press(chord);
    }
    await expect
      .poll(async () => (await written(page, SESSIONS.shopClaude)).length)
      .toBeGreaterThanOrEqual(10);
    const data = (await written(page, SESSIONS.shopClaude)).join('|');
    expect(data).toContain('\x01'); // Ctrl+A
    expect(data).toContain('\x12'); // Ctrl+R (reverse search must not reload the webview)
    expect(data).toContain('\x06'); // Ctrl+F
    expect(data).toContain('\x17'); // Ctrl+W (pane.close outside a terminal)
    expect(data).toContain('\x0b'); // Ctrl+K (palette.open outside a terminal)
    expect(data).toContain('\x04'); // Ctrl+D (pane.split_right outside a terminal)
    expect(data).toContain('\x1bb'); // Alt+B = ESC b
    expect(data).toContain('\x1b[Z'); // Shift+Tab
    expect(data).toContain('\x00'); // Ctrl+Space
    expect(data).toContain('\x1c'); // Ctrl+\
    // No Kelta action fired and the page was not reloaded.
    await expect(page.getByTestId('palette')).toHaveCount(0);
    expect(await page.evaluate(() => window.__kelta!.stores.projects.loaded)).toBe(true);
  });

  test('typing and Shift+Enter: ESC CR in Claude panes, plain Enter elsewhere', async ({ page }) => {
    await boot(page);
    // Focused pane is the Claude session.
    await page.locator('[data-testid="pane"][data-focused="true"] .xterm-helper-textarea').focus();
    await page.keyboard.type('hi');
    await page.keyboard.press('Shift+Enter');
    await expect.poll(async () => (await written(page, SESSIONS.shopClaude)).join('')).toContain('hi\x1b\r');

    // The nvim pane passes Shift+Enter through (xterm sends a plain CR).
    await dispatch(page, 'pane.focus_right');
    await page.locator('[data-testid="pane"][data-focused="true"] .xterm-helper-textarea').focus();
    await page.keyboard.press('Shift+Enter');
    await expect.poll(async () => (await written(page, SESSIONS.shopNvim)).join('')).toContain('\r');
    expect((await written(page, SESSIONS.shopNvim)).join('')).not.toContain('\x1b\r');
  });

  test('Kelta chords are consumed in terminals and outside', async ({ page }) => {
    await boot(page);
    await page.locator('[data-testid="pane"][data-focused="true"] .xterm-helper-textarea').focus();
    await page.keyboard.press('Control+Shift+K');
    // A load, not an assertion: the palette chunk is lazy and takes seconds on a loaded machine (#485).
    await page.getByTestId('palette').waitFor();
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('palette')).toHaveCount(0);
    // Consumed: nothing leaked into the PTY (Ctrl+Shift+K would otherwise be Ctrl+K = 0x0b).
    expect((await written(page, SESSIONS.shopClaude)).join('')).not.toContain('\x0b');
  });

  test('Ctrl+Shift+F in a terminal opens the search bar on Linux (#525)', async ({ page }) => {
    await boot(page);
    await page.locator('[data-testid="pane"][data-focused="true"] .xterm-helper-textarea').focus();
    await page.keyboard.press('Control+Shift+F');
    await expect(page.getByTestId('terminal-search')).toBeVisible();
  });

  test('app prefix: prefix then key, unknown keys are swallowed, Esc disarms', async ({ page }) => {
    await boot(page);
    await page.locator('[data-testid="pane"][data-focused="true"] .xterm-helper-textarea').focus();
    await page.keyboard.press('Control+Shift+Space');
    await expect(page.getByTestId('status-prefix')).toBeVisible();
    await page.keyboard.press('c'); // session.new
    await expect(page.getByTestId('new-session')).toBeVisible();
    await expect(page.getByTestId('status-prefix')).toHaveCount(0);
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('new-session')).toHaveCount(0);

    await page.locator('[data-testid="pane"][data-focused="true"] .xterm-helper-textarea').focus();
    await page.keyboard.press('Control+Shift+Space');
    await page.keyboard.press('q'); // unknown: swallowed
    await expect(page.getByTestId('status-prefix')).toHaveCount(0);
    expect((await written(page, SESSIONS.shopClaude)).join('')).not.toContain('q');

    // which-key (#416): after keys.prefix_hint_ms (400 ms) a panel lists the keys, the prefix stays armed
    // past keys.prefix_timeout_ms (1000 ms) until a key or Esc.
    await page.keyboard.press('Control+Shift+Space');
    const panel = page.getByTestId('which-key');
    await expect(panel).toBeVisible();
    await expect(panel.locator('[data-group="work"] h5')).toHaveText('This work: SHOP-142');
    await expect(panel.locator('[data-group="panes"]')).toContainText('Split right');
    await expect(panel.locator('[data-group="work"] dt').first()).toHaveText('↵');
    await expect(panel.locator('[data-group="goto"] dt')).toContainText(['0', '1–9']);
    await page.waitForTimeout(1200);
    await expect(page.getByTestId('status-prefix')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(panel).toHaveCount(0);
    await expect(page.getByTestId('status-prefix')).toHaveCount(0);
    await page.keyboard.press('x');
    await expect.poll(async () => (await written(page, SESSIONS.shopClaude)).join('')).toContain('x');
  });

  test('which-key panel: no work group off a work tab, ? opens Settings › Keys', async ({ page }) => {
    await boot(page);
    await unfold(page, 'shop');
    await page.locator('[data-testid="rail-row"][data-tab-id="shop-t2"]').click();
    await page.locator('[data-testid="pane"][data-focused="true"] .xterm-helper-textarea').focus();
    await page.keyboard.press('Control+Shift+Space');
    const panel = page.getByTestId('which-key');
    await expect(panel).toBeVisible();
    await expect(panel.locator('[data-group="goto"]')).toContainText('Tickets');
    await expect(panel.locator('[data-group="work"]')).toHaveCount(0);
    await page.keyboard.press('?');
    await expect(panel).toHaveCount(0);
    await expect(page.locator('[data-testid="settings-section"][data-section="keys"]')).toBeVisible();
  });

  test('which-key panel: the next step is labelled by the phase (SHOP-155 to review)', async ({ page }) => {
    await boot(page);
    for (let i = 0; i < 3; i++) await dispatch(page, 'attention.next'); // billing, billing error, then SHOP-155
    await expect(page.getByTestId('work-header')).toHaveAttribute('data-phase', 'to_review');
    await page.locator('[data-testid="pane"][data-focused="true"] .xterm-helper-textarea').focus();
    await page.keyboard.press('Control+Shift+Space');
    await expect(page.getByTestId('which-key').locator('[data-group="work"] dd').first()).toHaveText(
      'Review changes',
    );
  });

  test('⌘↵ in the Claude pane runs the next step (SHOP-155: Review changes, #441)', async ({ page }) => {
    await boot(page);
    for (let i = 0; i < 3; i++) await dispatch(page, 'attention.next'); // billing, billing error, then SHOP-155
    await expect(page.getByTestId('work-header')).toHaveAttribute('data-phase', 'to_review');
    await expect(page.locator('.work-header span.primary')).toHaveAttribute(
      'title',
      'Review changes (Ctrl+Enter)',
    );
    await page.locator('[data-testid="pane"][data-focused="true"] .xterm-helper-textarea').focus();
    await page.keyboard.press('Control+Enter');
    await expect.poll(async () => (await callsOf(page, 'work_diff')).length).toBe(1);
  });

  test('prefix pane actions: split and zoom', async ({ page }) => {
    await boot(page);
    await page.locator('[data-testid="pane"][data-focused="true"] .xterm-helper-textarea').focus();
    await page.keyboard.press('Control+Shift+Space');
    await page.keyboard.type('%');
    await expect(page.getByTestId('pane')).toHaveCount(3);
    await page.keyboard.press('Control+Shift+Enter'); // pane.zoom
    await expect(page.locator('[data-testid="pane"]:visible')).toHaveCount(1);
  });

  test('webview defaults are disabled: F5 does not reload, the context menu is suppressed', async ({
    page,
  }) => {
    await boot(page);
    await page.evaluate(() => ((window as unknown as { __marker: number }).__marker = 1));
    await page.getByTestId('rail').click({ position: { x: 20, y: 300 } });
    await page.keyboard.press('F5');
    await page.keyboard.press('Control+r');
    const prevented = await page.evaluate(() => {
      const e = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
      document.querySelector('[data-testid="statusbar"]')!.dispatchEvent(e);
      return e.defaultPrevented;
    });
    expect(prevented).toBe(true);
    await page.waitForTimeout(300);
    expect(await page.evaluate(() => (window as unknown as { __marker: number }).__marker)).toBe(1);
  });

  test('macOS chords: Cmd+K opens the palette, Ctrl+Shift+K does not', async ({ page }) => {
    await boot(page, 'macos');
    // A load, not an assertion: the palette chunk is lazy and takes seconds on a loaded machine (#485).
    await page.keyboard.press('Meta+k');
    await page.getByTestId('palette').waitFor();
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('palette')).toHaveCount(0);
    // The chunk is loaded now: a Ctrl+Shift+K that opened the palette would show at once.
    await page.keyboard.press('Control+Shift+K');
    await expect(page.getByTestId('palette')).toHaveCount(0);
  });
});
