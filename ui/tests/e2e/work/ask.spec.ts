// MCP `ask` (#356) on the IPC mock: the question's options as buttons on the ticket row, in Now and
// in Peek; the Tickets list shows the question above them, the ticket detail its body too (#375); a button types the option and Enter; a prompt open in the session disables them; typing
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
  // The row's bar (the selected row's brief has its own, #414).
  const bar = page.locator(`.askline [data-testid="ask"][data-session-id="${SID}"]`);
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
  await expect(page.locator('.ask-q')).toHaveText('Which candidate?');
  await page.locator('[data-key="jira-acme:SHOP-142"] .k-row-key').click();
  const detail = page.getByTestId('ticket-detail').getByTestId('ask');
  await expect(detail).toContainText('Which candidate?');
  await expect(detail).toContainText('Three candidates');
  await expect(detail.getByRole('button')).toHaveText(['Alpha', 'Beta', 'Gamma']);
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

test('the Queue brief answers the question with 1 and the row leaves Your move (#414)', async ({ page }) => {
  // The retry-queue spike's Claude asks (live, hooks on, inside its `ask` call).
  const spikeClaude = '0199a6b2-0000-7000-8000-000000000012';
  const row = '[data-row="w:0199a6b2-0000-7000-8000-00000000a005"]';
  await page.setViewportSize({ width: 1280, height: 1200 });
  await boot(page);
  await page.evaluate((id) => {
    const mock = window.__keltaMock!;
    const s = mock.state.sessions.find((x) => x.id === id)!;
    Object.assign(s, { lifecycle: 'live', status: 'working' });
    s.claude!.question = {
      question: 'Keep 3 tries?',
      options: ['Keep 3', 'Make it configurable'],
      body_html: '<p>The <strong>backoff</strong> constant</p>',
    };
    mock.emit({ type: 'session.updated', session: structuredClone(s) });
  }, spikeClaude);
  await page.keyboard.press('Control+Shift+Digit0');
  const now = page.getByTestId('inbox-pane');
  await expect(now.locator(`.row.yours${row}`)).toBeVisible();
  await now.locator(row).click();
  const ask = page.getByTestId('now-brief').getByTestId('ask');
  await expect(ask).toContainText('backoff constant');
  await expect(ask.getByRole('button', { name: 'Keep 3' })).toContainText('1');

  await page.keyboard.press('1');
  await expect
    .poll(() =>
      callsOf(page, 'session_write').then((calls) =>
        calls
          .filter((c) => c.args?.id === spikeClaude)
          .map((c) => String.fromCharCode(...Object.values(c.args?.data as Record<string, number>))),
      ),
    )
    .toEqual(['\x1b[200~Keep 3\x1b[201~', '\r']);
  await expect(now.locator(`.row.claude${row}`)).toBeVisible();
});
