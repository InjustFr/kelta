// Terminal panes: exit banner, clipboard, search, attach errors, hooks badge, theme push, welcome
// pane and the per-pane error boundary.
import { expect, test, type Page } from '@playwright/test';

import { boot, callsOf, dispatch, SESSIONS, waitForAttached } from './helpers';

async function openPane(page: Page, content: Record<string, unknown>): Promise<void> {
  await page.evaluate((c) => {
    window.__kelta!.stores.layout.open('shop', {
      content: c as never,
      placement: 'new_tab',
      focus: true,
      tab_title: null,
      work_item_id: null,
    });
  }, content);
}

test.describe('terminal pane', () => {
  test('renders the snapshot in the DOM renderer and sizes the PTY to the pane', async ({ page }) => {
    await boot(page);
    const pane = page.locator(`[data-session-id="${SESSIONS.shopClaude}"]`);
    await expect(pane.locator('.xterm-rows')).toContainText('kelta mock session claude');
    const attach = (await callsOf(page, 'session_attach')).find((c) => c.args?.id === SESSIONS.shopClaude)!;
    const { cols, rows } = attach.args as { cols: number; rows: number };
    expect(cols).toBeGreaterThan(20);
    expect(rows).toBeGreaterThan(5);
    // Output after the snapshot streams in and is acked once per frame (batched).
    await page.evaluate(
      (id) => window.__keltaMock!.output(id, 'line one\r\nline two\r\n'),
      SESSIONS.shopClaude,
    );
    await expect(pane.locator('.xterm-rows')).toContainText('line two');
    await expect.poll(async () => (await callsOf(page, 'session_ack')).length).toBeGreaterThan(0);
    const acked = (await callsOf(page, 'session_ack')).reduce(
      (n, c) => n + (c.args as { bytes: number }).bytes,
      0,
    );
    expect(acked).toBeGreaterThan(0);
  });

  test('resizing the window resizes the PTY once per frame', async ({ page }) => {
    await boot(page);
    const before = (await callsOf(page, 'session_resize')).length;
    await page.setViewportSize({ width: 900, height: 600 });
    await expect.poll(async () => (await callsOf(page, 'session_resize')).length).toBeGreaterThan(before);
    const last = (await callsOf(page, 'session_resize')).at(-1)!.args as { cols: number };
    const attach = (await callsOf(page, 'session_attach'))[0]!.args as { cols: number };
    expect(last.cols).toBeLessThan(attach.cols);
  });

  test('exit banner: Enter restarts the session and re-attaches', async ({ page }) => {
    await boot(page);
    await dispatch(page, 'project.goto.2');
    const pane = page.locator(`[data-session-id="${SESSIONS.billingExited}"]`);
    await expect(pane.getByTestId('exit-banner')).toContainText('Exited');
    await page.locator(`[data-pane-id="billing-p3"]`).click();
    await page.locator(`[data-session-id="${SESSIONS.billingExited}"] .xterm-helper-textarea`).focus();
    await page.keyboard.press('Enter');
    await expect.poll(async () => (await callsOf(page, 'session_restart')).length).toBe(1);
    await expect(pane.getByTestId('exit-banner')).toHaveCount(0);
    const attaches = (await callsOf(page, 'session_attach')).filter(
      (c) => c.args?.id === SESSIONS.billingExited,
    );
    expect(attaches.length).toBe(2);
  });

  test('exit banner: x closes the pane without killing anything', async ({ page }) => {
    await boot(page);
    await dispatch(page, 'project.goto.2');
    await expect(
      page.locator(`[data-session-id="${SESSIONS.billingExited}"]`).getByTestId('exit-banner'),
    ).toBeVisible();
    await page.locator(`[data-session-id="${SESSIONS.billingExited}"] .xterm-helper-textarea`).focus();
    await page.keyboard.press('x');
    await expect(page.locator(`[data-session-id="${SESSIONS.billingExited}"]`)).toHaveCount(0);
    expect((await callsOf(page, 'session_kill')).length).toBe(0);
  });

  test('copy writes the selection through clipboard_write; paste reads clipboard_read', async ({ page }) => {
    await boot(page);
    const text = await page.evaluate(async (id) => {
      const { terminalPool } = await import(/* @vite-ignore */ '/src/lib/terminal/index.ts' as string);
      const view = terminalPool.get(id) as unknown as { term: { selectAll(): void } };
      view.term.selectAll();
      await window.__kelta!.actions.dispatch('terminal.copy');
      return true;
    }, SESSIONS.shopClaude);
    expect(text).toBe(true);
    const write = (await callsOf(page, 'clipboard_write'))[0]!;
    expect(write.args).toMatchObject({ kind: 'clipboard' });
    expect(String((write.args as { text: string }).text)).toContain('kelta mock');

    await page.locator('[data-testid="pane"][data-focused="true"] .xterm-helper-textarea').focus();
    await dispatch(page, 'terminal.paste');
    await expect.poll(async () => (await callsOf(page, 'clipboard_read')).length).toBe(1);
    expect((await callsOf(page, 'clipboard_read'))[0]!.args).toMatchObject({ kind: 'clipboard' });
    // The mock clipboard text is a single line: pasted without confirmation.
    await expect
      .poll(async () =>
        (await callsOf(page, 'session_write')).some((c) => {
          const d = c.args?.data as Record<string, number>;
          return String.fromCharCode(...Object.values(d)).includes('mock clipboard text');
        }),
      )
      .toBe(true);
  });

  test('multi-line paste into a prompt without bracketed paste asks first', async ({ page }) => {
    await boot(page);
    const done = page.evaluate(async (id) => {
      const { terminalPool } = await import(/* @vite-ignore */ '/src/lib/terminal/index.ts' as string);
      const view = terminalPool.get(id) as unknown as { pasteText(t: string): Promise<void> };
      await view.pasteText('echo one\necho two\n');
      return true;
    }, SESSIONS.shopClaude);
    await expect(page.getByTestId('confirm-dialog')).toContainText('2 lines');
    await page.getByTestId('confirm-paste').click();
    expect(await done).toBe(true);
    await expect
      .poll(async () =>
        (await callsOf(page, 'session_write'))
          .map((c) => String.fromCharCode(...Object.values(c.args?.data as Record<string, number>)))
          .join(''),
      )
      .toContain('echo one\recho two\r');
  });

  test('bracketed paste markers inside pasted text are stripped, and cancel pastes nothing', async ({
    page,
  }) => {
    await boot(page);
    const done = page.evaluate(async (id) => {
      const { terminalPool } = await import(/* @vite-ignore */ '/src/lib/terminal/index.ts' as string);
      const view = terminalPool.get(id) as unknown as { pasteText(t: string): Promise<void> };
      await view.pasteText('a\nb\x1b[201~rm -rf /\n');
      return true;
    }, SESSIONS.shopClaude);
    await expect(page.getByTestId('confirm-dialog')).toBeVisible();
    await page.keyboard.press('Escape');
    await done;
    const sent = (await callsOf(page, 'session_write'))
      .map((c) => String.fromCharCode(...Object.values(c.args?.data as Record<string, number>)))
      .join('');
    expect(sent).not.toContain('rm -rf');
  });

  test('search bar loads the addon lazily and flags misses', async ({ page }) => {
    await boot(page);
    const requested: string[] = [];
    page.on('request', (r) => {
      if (/addon-search/.test(r.url())) requested.push(r.url());
    });
    expect(requested).toEqual([]);
    await dispatch(page, 'terminal.search');
    const bar = page.getByTestId('terminal-search');
    await expect(bar).toBeVisible();
    await bar.getByRole('textbox').fill('kelta');
    await expect.poll(() => requested.length).toBeGreaterThan(0);
    await expect(bar.getByRole('textbox')).not.toHaveClass(/miss/);
    await bar.getByRole('textbox').fill('zzzzzzzz');
    await expect(bar.getByRole('textbox')).toHaveClass(/miss/);
    await page.keyboard.press('Escape');
    await expect(bar).toHaveCount(0);
  });

  test('an attach failure shows the error with Diagnostics and Retry', async ({ page }) => {
    await boot(page);
    await page.evaluate(() =>
      window.__keltaMock!.failNext('session_attach', {
        code: 'not_found',
        message: '`claude` not found in your login PATH',
      }),
    );
    await dispatch(page, 'project.goto.3'); // kelta-tools: its Claude pane attaches for the first time
    const error = page.getByTestId('terminal-error');
    await expect(error).toContainText('not found in your login PATH');
    await expect(error.getByRole('button', { name: 'Open diagnostics' })).toBeVisible();
    await error.getByRole('button', { name: 'Retry' }).click();
    await expect(error).toHaveCount(0);
    await waitForAttached(page, 1);
  });

  test('pane header shows the hooks-inactive badge for Claude sessions', async ({ page }) => {
    await boot(page);
    await page.evaluate((id) => {
      const mock = window.__keltaMock!;
      const s = mock.state.sessions.find((x) => x.id === id)!;
      mock.emit({
        type: 'session.updated',
        session: { ...s, claude: { ...s.claude!, hooks_active: false } },
      });
    }, SESSIONS.shopClaude);
    const badge = page.getByTestId('hooks-badge').first();
    await expect(badge).toContainText('Live status off');
    await expect(page.getByTestId('status-hooks')).toContainText('Live status off');
    const tabs = await page.getByTestId('tab').count();
    await badge.getByRole('button', { name: 'Fix' }).click();
    await expect(page.getByTestId('tab')).toHaveCount(tabs + 1);
  });

  test('theme changes re-theme terminals and push the palette to Rust', async ({ page }) => {
    await boot(page);
    const first = (await callsOf(page, 'terminal_set_palette'))[0]!.args as {
      palette: { background: string };
    };
    expect(first.palette.background).toMatch(/^#[0-9a-f]{6}$/);
    await page.evaluate(async () => {
      const { settingsSet } = await import(/* @vite-ignore */ '/src/lib/ipc/commands.ts' as string);
      await settingsSet({ layer: 'global', path: 'app.theme', value: 'light' });
    });
    await expect.poll(() => page.evaluate(() => document.documentElement.dataset.theme)).toBe('light');
    await expect
      .poll(
        async () =>
          ((await callsOf(page, 'terminal_set_palette')).at(-1)!.args as { palette: { background: string } })
            .palette.background,
      )
      .toBe('#fafbfb');
    await page.evaluate(async () => {
      const { settingsSet } = await import(/* @vite-ignore */ '/src/lib/ipc/commands.ts' as string);
      await settingsSet({ layer: 'global', path: 'app.theme', value: 'dark' });
    });
    await expect
      .poll(
        async () =>
          ((await callsOf(page, 'terminal_set_palette')).at(-1)!.args as { palette: { background: string } })
            .palette.background,
      )
      .toBe('#121519');
  });

  test('a selected theme recolours chrome, open terminals and the palette live', async ({ page }) => {
    await boot(page);
    const set = (path: string, value: unknown) =>
      page.evaluate(
        async ([path, value]) => {
          const { settingsSet } = await import(/* @vite-ignore */ '/src/lib/ipc/commands.ts' as string);
          await settingsSet({ layer: 'global', path, value });
        },
        [path, value] as const,
      );
    const lastPalette = async () =>
      ((await callsOf(page, 'terminal_set_palette')).at(-1)!.args as { palette: { background: string } })
        .palette.background;
    const css = (name: string) =>
      page.evaluate((n) => getComputedStyle(document.documentElement).getPropertyValue(n).trim(), name);
    const inline = () => page.evaluate(() => document.documentElement.style.cssText);
    const viewBg = () =>
      page.evaluate(async (id) => {
        const { terminalPool } = await import(/* @vite-ignore */ '/src/lib/terminal/index.ts' as string);
        const view = terminalPool.get(id) as unknown as {
          term: { options: { theme: { background: string } } };
        };
        return view.term.options.theme.background;
      }, SESSIONS.shopClaude);
    await set('app.theme', 'dark');
    await expect.poll(lastPalette).toBe('#121519');
    const bezel = await css('--k-bezel');
    const before = await inline();

    await set('themes.x', {
      name: 'X',
      base: 'dark',
      ui: { well: '#002B36', fg: '#93a1a1' },
      terminal: {},
    });
    await set('app.dark_theme', 'x');
    await expect.poll(lastPalette).toBe('#002b36');
    expect(await css('--k-well')).toBe('#002b36');
    expect(await css('--k-term-bg')).toBe('#002b36');
    expect(await css('--k-fg')).toBe('#93a1a1');
    expect(await css('--k-bezel')).toBe(bezel); // unset tokens keep the Bezel value
    await expect.poll(viewBg).toBe('#002b36');

    await set('app.dark_theme', 'bezel-dark');
    await expect.poll(lastPalette).toBe('#121519');
    expect(await inline()).toBe(before); // every inline var the theme set is gone
    expect(await css('--k-well')).toBe('#121519');
    await expect.poll(viewBg).toBe('#121519');
  });

  test('welcome pane lists the environment checks', async ({ page }) => {
    await boot(page);
    await openPane(page, { kind: 'welcome' });
    const welcome = page.getByTestId('welcome-pane');
    await expect(welcome).toBeVisible();
    await expect(welcome.getByText('Your environment')).toBeVisible();
    await expect(welcome.locator('li').first()).toBeVisible();
    await expect(welcome.getByRole('button', { name: 'Create project from folder' })).toBeVisible();
  });

  test('a crashing pane shows an inline error and leaves the others running', async ({ page }) => {
    await boot(page);
    await page.evaluate(() => {
      const registry = window.__kelta!.registry.paneRegistry as Record<
        string,
        () => Promise<{ default: unknown }>
      >;
      registry.diagnostics = () =>
        Promise.resolve({
          default: function Boom() {
            throw new Error('boom from diagnostics');
          },
        });
    });
    await openPane(page, { kind: 'diagnostics' });
    await expect(page.getByText('This pane crashed')).toBeVisible();
    await expect(page.getByText('boom from diagnostics')).toBeVisible();
    // The app (and the terminals of other tabs) are unaffected.
    await expect(page.getByTestId('tabbar')).toBeVisible();
    await page.getByRole('button', { name: 'Close pane' }).last().click();
    await page.getByTestId('tab').first().click();
    await waitForAttached(page, 2);
  });
});

test('a killed session leaves a closable placeholder instead of a dead terminal', async ({ page }) => {
  await boot(page);
  await page.evaluate((id) => window.__keltaMock!.emit({ type: 'session.removed', id }), SESSIONS.shopNvim);
  const gone = page.getByTestId('terminal-missing');
  await expect(gone).toContainText('This session is gone');
  // No attach is attempted for a session that does not exist.
  expect(
    (await callsOf(page, 'session_attach')).filter((c) => c.args?.id === SESSIONS.shopNvim),
  ).toHaveLength(1);
  await gone.getByRole('button', { name: 'Close pane' }).click();
  await expect(page.getByTestId('pane')).toHaveCount(1);
});
