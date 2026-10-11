// MCP `ask` (#356) on the IPC mock: the question's options as buttons on the ticket row, in Now and
// in Peek; a button types the option and Enter; a prompt open in the session disables them; typing
// in the terminal clears the question too.
import { expect, test, type Page } from '@playwright/test';

import { boot, callsOf, dispatch, SESSIONS } from '../shell/helpers';

const SID = SESSIONS.shopClaude;

async function ask(page: Page, status = 'working'): Promise<void> {
  await page.evaluate(
    ([id, status]) => {
      const mock = window.__keltaMock!;
      const s = mock.state.sessions.find((x) => x.id === id)!;
      s.status = status as typeof s.status;
      s.claude!.question = {
        question: 'Which candidate?',
        options: ['Alpha', 'Beta', 'Gamma'],
        body_html: '<p>Three <strong>candidates</strong></p>',
      };
      mock.emit({ type: 'session.updated', session: structuredClone(s) });
    },
    [SID, status] as const,
  );
}

const writes = (page: Page) =>
  callsOf(page, 'session_write').then((calls) =>
    calls
      .filter((c) => c.args?.id === SID)
      .map((c) => String.fromCharCode(...Object.values(c.args?.data as Record<string, number>))),
  );

test('Now and the ticket row show the options; a button answers and clears the question', async ({
  page,
}) => {
  await boot(page);
  await ask(page);

  await page.getByTestId('rail-inbox').click();
  const bar = page.locator(`[data-testid="ask"][data-session-id="${SID}"]`);
  await expect(bar.getByRole('button')).toHaveText(['Alpha', 'Beta', 'Gamma']);
  await expect(page.locator('[data-row="w:0199a6b2-0000-7000-8000-00000000a001"]')).toContainText(
    'Which candidate?',
  );

  // A permission prompt in the same session: Enter would answer it, so the buttons wait.
  await ask(page, 'needs_input');
  for (const b of await bar.getByRole('button').all()) await expect(b).toBeDisabled();
  await ask(page, 'working');
  await expect(bar.getByRole('button', { name: 'Beta' })).toBeEnabled();

  await dispatch(page, 'tickets.open', { project_id: 'shop' });
  await page.getByRole('tab', { name: 'List' }).click();
  await expect(page.locator('[data-key="jira-acme:SHOP-142"]')).toBeVisible();
  const row = page.locator(`.ask-line [data-testid="ask"][data-session-id="${SID}"]`);
  await expect(row.getByRole('button')).toHaveText(['Alpha', 'Beta', 'Gamma']);
  await row.getByRole('button', { name: 'Beta' }).click();
  await expect.poll(() => writes(page)).toEqual(['\x1b[200~Beta\x1b[201~', '\r']);
  await expect(page.getByTestId('ask')).toHaveCount(0);
});

test('Peek shows the body; typing in the terminal clears the question', async ({ page }) => {
  await boot(page);
  await ask(page);
  await page.locator('[data-testid="rail-row"][data-tab-id="shop-t1"]').hover({ position: { x: 20, y: 10 } });
  const peek = page.getByTestId('peek');
  await expect(peek).toHaveAttribute('data-session-id', SID);
  await expect(peek.getByTestId('ask')).toContainText('Three candidates');
  await expect(peek.getByTestId('ask').getByRole('button')).toHaveText(['Alpha', 'Beta', 'Gamma']);
  await page.mouse.move(0, 0);
  await page.mouse.click(1, 400);

  await dispatch(page, 'session.reveal', { session_id: SID });
  const term = page.locator(
    `[data-testid="pane"][data-focused="true"] [data-session-id="${SID}"] .xterm-helper-textarea`,
  );
  await term.focus();
  await page.keyboard.type('x');
  await expect
    .poll(() => page.evaluate((id) => window.__kelta!.stores.sessions.get(id)?.claude?.question ?? null, SID))
    .toBeNull();
});
