// Fix-loop e2e on the IPC mock: Flow 2 (feedback into the previous Claude conversation, keyboard
// only) and a conflicted rebase (stopped → abort, then resolved → force push confirmation).
import { expect, test, type Page } from '@playwright/test';

const HARNESS = '/tests/e2e/work/harness/index.html';
const SUBMIT = process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter';

/** Boots the harness on the kelta-tools project, whose first tab is the gh-12 work item (PR #13). */
async function boot(page: Page): Promise<string[]> {
  const errors: string[] = [];
  page.on('pageerror', (err) => errors.push(err.message));
  page.on('console', (msg) => {
    if (msg.type() === 'error') errors.push(msg.text());
  });
  await page.goto(HARNESS);
  await page.waitForFunction(() => window.__kelta?.stores.projects.loaded === true);
  await page.evaluate(async () => {
    const k = window.__kelta!.stores;
    await k.bootstrap();
    await k.projects.activate('kelta-tools');
    await k.layout.ensure('kelta-tools');
  });
  await expect(page.getByTestId('work-header')).toContainText('feat/gh-12-json-output');
  return errors;
}

const lastCall = (page: Page, cmd: string) =>
  page.evaluate((c) => window.__keltaMock!.calls.filter((x) => x.cmd === c).at(-1)?.args ?? null, cmd);

test('Flow 2: Fix with Claude sends the checked feedback into the previous conversation', async ({ page }) => {
  const errors = await boot(page);
  await page.getByTestId('work-header').getByRole('button', { name: 'Fix with Claude' }).click();
  const sheet = page.getByRole('dialog', { name: 'Fix with Claude' });
  await expect(sheet).toBeVisible();
  const items = sheet.getByTestId('fix-item');
  await expect(items).toHaveCount(3);
  await expect(sheet.getByLabel('Prompt')).toBeFocused();
  await expect(sheet.getByTestId('fix-resumes')).toContainText('Resumes conversation');

  // Keyboard: Tab into the list, j to the review summary, Space unchecks it.
  await items.first().focus();
  await page.keyboard.press('j');
  await expect(items.nth(1)).toBeFocused();
  await page.keyboard.press('Space');
  await expect(items.nth(1)).not.toBeChecked();

  await page.keyboard.press(SUBMIT);
  await expect(sheet).toBeHidden();
  await expect(page.getByTestId('toasts')).toContainText('Sent 2 feedback items to Claude.');
  const sent = (await lastCall(page, 'work_send')) as {
    prompt: string;
    files: { name: string; content: string }[];
    threads: string[];
  };
  expect(sent.files.map((f) => f.name)).toEqual(['feedback.md']);
  expect(sent.files[0]!.content).toContain('Reset the counter after a successful login.');
  expect(sent.files[0]!.content).not.toContain('Close, two things to fix.');
  expect(sent.threads).toEqual(['PRRT_kwDOA1']);
  expect(sent.prompt).toContain('{file}');

  // Claude is now working: a second send is refused in the sheet, nothing is typed.
  await page.getByTestId('work-header').getByRole('button', { name: 'Fix with Claude' }).click();
  await expect(sheet.getByText('Claude is busy; send when it stops.')).toBeVisible();
  await expect(sheet.getByRole('button', { name: /Send to Claude/ })).toBeDisabled();
  await page.keyboard.press('Escape');
  expect(errors).toEqual([]);
});

test('a conflicted rebase stops, aborts, then resolves into a confirmed force push', async ({ page }) => {
  const errors = await boot(page);
  const header = page.getByTestId('work-header');
  await header.getByRole('button', { name: 'Rebase onto main' }).click();
  await expect(header).toContainText('Rebase stopped (1 conflicted file)');
  await expect(page.getByTestId('toasts')).toContainText('Rebase stopped: 1 conflicted file.');

  await header.getByRole('button', { name: 'Abort rebase' }).click();
  await expect(header).not.toContainText('Rebase stopped');
  expect(await lastCall(page, 'work_rebase')).toMatchObject({ op: { kind: 'abort' } });

  await header.getByRole('button', { name: 'Rebase onto main' }).click();
  await header.getByRole('button', { name: 'Ask Claude to resolve' }).click();
  const brief = (await lastCall(page, 'work_send')) as { files: { name: string; content: string }[] };
  expect(brief.files[0]!.name).toBe('conflicts.md');
  expect(brief.files[0]!.content).toContain('- `src/output.rs`');
  // Claude resolves and stops (Stop hook): idle again.
  await page.evaluate(() => {
    const m = window.__keltaMock!;
    const s = m.state.sessions.find((x) => x.kind.type === 'claude' && x.project_id === 'kelta-tools')!;
    s.status = 'done';
    m.emit({ type: 'session.updated', session: structuredClone(s) });
  });

  await header.getByRole('button', { name: 'Continue' }).click();
  await expect(header).toContainText('Rebased (push rewrites #13)');
  await header.getByRole('button', { name: 'Force push…' }).click();
  const dialog = page.getByRole('dialog', { name: 'Force push' });
  await expect(dialog).toContainText(/The lease checks origin is still at \w{7}\./);
  expect(await lastCall(page, 'work_push')).toBeNull();
  await page.keyboard.press(SUBMIT);
  await expect(dialog).toBeHidden();
  expect(await lastCall(page, 'work_push')).toMatchObject({ force: true });
  await expect(page.getByTestId('toasts')).toContainText('Force pushed feat/gh-12-json-output.');
  await expect(header).not.toContainText('Rebased (push rewrites');
  expect(errors).toEqual([]);
});
