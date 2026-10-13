// Run a saved workflow on a ticket (#233) on the IPC mock: `w` in the detail, the picker, the run in
// a background tab, the toast's Open and Peek, the ticket row's run chip, and the run's live view (#272),
// split beside the session's terminal in its own tab (#392).
import { expect, test } from '@playwright/test';

import { boot, callsOf, dispatch, unfold } from '../shell/helpers';

test('detail w: the run opens a background tab, the toast opens and peeks its session', async ({ page }) => {
  await boot(page);
  await dispatch(page, 'tickets.open', { project_id: 'shop' });
  await page.getByRole('tab', { name: 'List' }).click(); // the mock layout opens the board
  const row = page.locator('[data-key="jira-acme:SHOP-151"]');
  await row.locator('.k-row-key').click();
  await page.keyboard.press('g'); // Enter starts work now (#417); g opens the detail
  const detail = page.getByTestId('ticket-detail');
  await expect(detail.getByRole('heading', { level: 1 })).toHaveText('Checkout: show tax breakdown');
  // The opened detail is saved first: a backend layout change drops a pending local save.
  await page.evaluate(() => window.__kelta!.stores.layout.flush());
  await detail.focus();
  await page.keyboard.press('w');
  const picker = page.getByTestId('workflow-picker');
  await expect(picker).toContainText('Implement, review and merge one ticket');
  await expect(picker).toContainText('triage');
  await page.keyboard.press('Enter');

  const toast = page.getByTestId('toasts');
  await expect(toast).toContainText('kelta-ticket-pass started on SHOP-151');
  const [run] = await callsOf(page, 'workflow_run');
  expect(run?.args).toMatchObject({ ticket: { key: 'SHOP-151' }, workflow: 'kelta-ticket-pass' });
  await expect(detail).toBeFocused();
  // The run's tab is no work tab: it folds under shop's "n more" (#430).
  await unfold(page, 'shop');
  const tab = page.locator('[data-testid="rail-row"]', { hasText: 'kelta-ticket-pass · SHOP-151' });
  await expect(tab).toHaveCount(1);
  // The row puts the run with Claude and names it in its tooltip (#400); the detail names it too.
  await expect(row).toHaveAttribute('title', /kelta-ticket-pass · Starting/);
  await expect(detail.locator('[data-run]')).toContainText('kelta-ticket-pass · Starting');

  const sid = await page.evaluate(() => {
    const args = window.__kelta!.stores.toasts.list.at(-1)!.toast.action!.args as { session_id: string };
    return args.session_id;
  });
  await toast.getByRole('button', { name: 'Peek' }).click();
  await expect(page.getByTestId('peek')).toHaveAttribute('data-session-id', sid);
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('peek')).toHaveCount(0);

  // Open (the toast's action, now dismissed) reveals the session's pane.
  await dispatch(page, 'session.reveal', { session_id: sid });
  await expect(
    page.locator(`[data-testid="pane"][data-focused="true"] [data-session-id="${sid}"]`),
  ).toHaveCount(1);

  // #272: the run's hooks show in the context bar; its pane lists phases and agents, live.
  // #392: the run claimed the ticket, so the rail shows it on the ticket, not on the session's row.
  await expect(tab).toHaveCount(1);
  await expect(page.getByTestId('rail-run')).toHaveCount(0);
  const chip = page.getByTestId('context-workflow');
  await expect(chip).toHaveText('⚙ kelta-ticket-pass · Pick');
  await chip.click();
  const sheet = page.locator('[data-testid="pane"][data-focused="true"]').getByTestId('workflow-runs');
  await expect(page.getByTestId('pane')).toHaveCount(2); // the terminal and its runs, in one tab
  await expect(page.getByRole('dialog')).toHaveCount(0);
  const agents = sheet.getByTestId('workflow-agent');
  await expect(agents).toHaveText(['●pick']);
  await agents.first().click();
  await expect(sheet.getByTestId('workflow-tail')).toContainText('Bash: Run the tests');
  await page.evaluate((session) => {
    const run = {
      run_id: `wf_${session.slice(0, 8)}-000`,
      session_id: session,
      name: 'kelta-ticket-pass',
      status: 'completed',
      phases: [
        { title: 'Pick', agents: [{ id: 'p', label: 'pick', done: true }] },
        { title: 'Implement', agents: [{ id: 'i', label: 'implement-#151', done: true }] },
      ],
      duration_ms: 90_000,
      total_tokens: 5000,
      total_tool_calls: 12,
      result: '{"status": "merged"}',
      logs: ['Picked #151'],
    };
    window.__keltaMock!.emit({ type: 'workflow.updated', run });
  }, sid);
  await expect(agents).toHaveText(['✓pick', '✓implement-#151']);
  await expect(sheet.getByTestId('workflow-run')).toContainText('1m 30s · 5,000 tokens · 12 tool calls');
  // Back on the terminal, the chip shows the final status; opening again focuses the same pane.
  await page.locator(`[data-testid="pane"] [data-session-id="${sid}"]`).click();
  await expect(chip).toHaveText('⚙ kelta-ticket-pass · completed');
  await chip.click();
  await expect(sheet).toHaveCount(1);
  await expect(page.getByTestId('pane')).toHaveCount(2);
});
