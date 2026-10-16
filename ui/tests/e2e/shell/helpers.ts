// Helpers shared by the shell e2e specs (mock IPC, deterministic Linux platform).
import { expect, type Page } from '@playwright/test';

export type Platform = 'linux' | 'macos';

/** Opens the app on the mock IPC pretending to run on `platform` (chords, renderer default). */
export async function boot(page: Page, platform: Platform = 'linux'): Promise<void> {
  await page.addInitScript((p) => {
    Object.defineProperty(Navigator.prototype, 'platform', {
      get: () => (p === 'linux' ? 'Linux x86_64' : 'MacIntel'),
    });
  }, platform);
  await page.goto('/');
  await page.waitForFunction(() => window.__kelta?.stores.projects.loaded === true);
  // A load, not an assertion: the lazy terminal chunk takes seconds on a loaded machine (#485).
  await page.getByTestId('workspace').waitFor();
  await waitForAttached(page, 2);
}

/** Opens a rail band's "n more" row (#430): the project's tabs besides its work rows. The compact
 * sidebar has none (#435): it is opened first. */
export async function unfold(page: Page, project: string): Promise<void> {
  if (await page.getByTestId('rail').evaluate((e) => e.classList.contains('compact')))
    await page.getByTestId('rail-toggle').click();
  const more = page.locator(`[data-testid="rail-more"][data-project-id="${project}"]`);
  if ((await more.getAttribute('aria-expanded')) !== 'true') await more.click();
}

/** Waits until `n` terminal panes in the DOM are attached to their sessions. */
export async function waitForAttached(page: Page, n: number): Promise<void> {
  const attached = page.locator('[data-testid="terminal-pane"][data-attached="true"]');
  // A load, not an assertion: the lazy terminal chunk and xterm take seconds on a loaded machine (#485).
  if (n > 0) await attached.nth(n - 1).waitFor();
  await expect(attached).toHaveCount(n);
}

export interface MockCall {
  cmd: string;
  args: Record<string, unknown> | undefined;
}

export async function callLog(page: Page): Promise<MockCall[]> {
  return page.evaluate(() => window.__keltaMock!.calls.map((c) => ({ cmd: c.cmd, args: c.args as never })));
}

export async function callsOf(page: Page, cmd: string): Promise<MockCall[]> {
  return (await callLog(page)).filter((c) => c.cmd === cmd);
}

export async function activeProject(page: Page): Promise<string | null> {
  return page.evaluate(() => window.__kelta!.stores.projects.activeId);
}

export const SESSIONS = {
  shopClaude: '0199a6b2-0000-7000-8000-000000000001',
  shopNvim: '0199a6b2-0000-7000-8000-000000000002',
  billingClaude: '0199a6b2-0000-7000-8000-000000000005',
  billingExited: '0199a6b2-0000-7000-8000-000000000007',
} as const;

export async function dispatch(page: Page, id: string, args?: Record<string, unknown>): Promise<void> {
  await page.evaluate(([i, a]) => window.__kelta!.actions.dispatch(i as string, a as never), [
    id,
    args,
  ] as const);
}

/** Boots and waits for the work tab's header and Story panel: their lazy chunks land after boot and
 * shrink the pane area (#483), so pane boxes read before them go stale (#487). */
export async function bootSettled(page: Page): Promise<void> {
  await boot(page);
  // Loads, not assertions: lazy chunks take seconds on a loaded machine (#485).
  await page.getByTestId('work-header').waitFor();
  await page.getByTestId('story-panel').waitFor();
}
