// L9 e2e on the IPC mock (VITE_IPC=mock): start work from the board, move a ticket (and roll back),
// approve a review, the Inbox "Other" group. The shell is L2's: the harness page hosts the panes.
import { expect, test, type Page } from '@playwright/test';

const HARNESS = '/tests/e2e/work/harness/index.html';

async function boot(page: Page): Promise<string[]> {
  const errors: string[] = [];
  page.on('pageerror', (err) => errors.push(err.message));
  page.on('console', (msg) => {
    if (msg.type() === 'error') errors.push(msg.text());
  });
  await page.goto(HARNESS);
  await page.waitForFunction(() => window.__kelta?.stores.projects.loaded === true);
  await page.evaluate(async () => {
    await window.__kelta!.stores.bootstrap();
  });
  return errors;
}

async function dispatch(page: Page, id: string, args?: Record<string, unknown>): Promise<void> {
  await page.evaluate(([i, a]) => window.__kelta!.actions.dispatch(i as string, a as never), [
    id,
    args,
  ] as const);
}

async function openBoard(page: Page): Promise<void> {
  await dispatch(page, 'tickets.open');
  await page.getByRole('tab', { name: 'Board' }).click();
  await expect(page.locator('[data-column]')).toHaveCount(4);
}

const card = (page: Page, key: string) => page.locator(`[data-key="jira-acme:${key}"]`);
const lane = (page: Page, id: string) => page.locator(`[data-column="${id}"]`);

test.describe('tickets board', () => {
  test('moves a ticket with the keyboard and survives a reload of the list', async ({ page }) => {
    const errors = await boot(page);
    await openBoard(page);
    await expect(lane(page, 'todo').locator('[data-key="jira-acme:SHOP-151"]')).toBeVisible();

    await card(page, 'SHOP-151').click();
    await page.keyboard.press('m');
    await expect(page.getByRole('menu')).toBeVisible();
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('Enter');
    await expect(lane(page, 'in_progress').locator('[data-key="jira-acme:SHOP-151"]')).toBeVisible();

    await page.keyboard.press('Shift+R'); // refresh from the backend
    await expect(lane(page, 'in_progress').locator('[data-key="jira-acme:SHOP-151"]')).toBeVisible();
    expect(errors).toEqual([]);
  });

  test('moves a ticket by dragging its card', async ({ page }) => {
    await boot(page);
    await openBoard(page);
    await card(page, 'SHOP-155').dragTo(lane(page, 'in_review'));
    await expect(lane(page, 'in_review').locator('[data-key="jira-acme:SHOP-155"]')).toBeVisible();
  });

  test('rolls a failed move back and toasts the error', async ({ page }) => {
    await boot(page);
    await openBoard(page);
    await card(page, 'SHOP-151').click();
    await page.evaluate(() =>
      window.__keltaMock!.failNext('tracker_move', { code: 'upstream', message: 'Jira said no' }),
    );
    await page.keyboard.press('Shift+ArrowRight');
    await expect(page.getByTestId('toasts')).toContainText('Jira said no');
    await expect(lane(page, 'todo').locator('[data-key="jira-acme:SHOP-151"]')).toBeVisible();
  });

  test('starts work from the board (keyboard only)', async ({ page }) => {
    const errors = await boot(page);
    await openBoard(page);
    await card(page, 'SHOP-151').click();
    await page.keyboard.press('s');
    const sheet = page.getByRole('dialog', { name: /Start work on SHOP-151/ });
    await expect(sheet).toBeVisible();
    await expect(sheet.getByLabel('Branch', { exact: true })).toHaveValue(/^feat\/shop-151/);
    await page.keyboard.press('Control+Enter');
    await expect(sheet).toBeHidden();
    await expect(page.getByTestId('toasts')).toContainText('Started feat/shop-151');
    const started = await page.evaluate(() =>
      window.__kelta!.stores.work.all.some((w) => w.ticket?.key === 'SHOP-151' && w.state.kind === 'active'),
    );
    expect(started).toBe(true);
    expect(errors).toEqual([]);
  });
});

test.describe('reviews', () => {
  test('approves a review with the displayed head_sha', async ({ page }) => {
    const errors = await boot(page);
    await dispatch(page, 'reviews.open');
    await expect(page.getByText('SHOP-150: cache product images')).toBeVisible();
    await page.keyboard.press('Enter'); // opens the detail of the first review
    const detail = page.getByTestId('review-detail');
    await expect(detail).toBeVisible();
    const shown = await detail.getByTestId('head-sha').getAttribute('title');
    await detail.getByRole('button', { name: 'Approve' }).click();
    await expect(page.getByTestId('toasts')).toContainText('Approved');
    const sent = await page.evaluate(
      () => window.__keltaMock!.calls.filter((c) => c.cmd === 'review_approve').at(-1)?.args,
    );
    expect(sent).toMatchObject({ head_sha: shown });
    expect(errors).toEqual([]);
  });
});

test.describe('inbox', () => {
  test('lists unbound review requests under Other', async ({ page }) => {
    const errors = await boot(page);
    await page.evaluate(() =>
      window.__kelta!.stores.layout.open('shop', {
        content: { kind: 'inbox' },
        placement: 'new_tab',
        focus: true,
        tab_title: 'Inbox',
        work_item_id: null,
      }),
    );
    const inbox = page.getByTestId('inbox-pane');
    await expect(inbox).toBeVisible();
    const requests = inbox.locator('[data-section="s:requested"]');
    await expect(requests).toBeVisible();
    await expect(inbox.getByText('Terraform: add read replica')).toBeVisible();
    // The "Other" group header sits right above the unbound request.
    const order = await inbox
      .locator('[data-group], [data-section], .k-row')
      .evaluateAll((els) =>
        els.map((e) => e.getAttribute('data-group') ?? e.getAttribute('data-section') ?? e.textContent ?? ''),
      );
    const at = order.findIndex((t, i) => t === 'Other' && (order[i + 1] ?? '').includes('Terraform'));
    expect(at).toBeGreaterThan(-1);
    // Needs-input sessions of other projects are listed first, each with the needs-input lamp.
    await expect(inbox.locator('[data-section="s:input"]')).toBeVisible();
    await expect(inbox.getByRole('img', { name: 'needs input' }).first()).toBeVisible();
    expect(errors).toEqual([]);
  });

  test('Enter on a ticket opens its detail', async ({ page }) => {
    await boot(page);
    await page.evaluate(() =>
      window.__kelta!.stores.layout.open('shop', {
        content: { kind: 'inbox' },
        placement: 'new_tab',
        focus: true,
        tab_title: 'Inbox',
        work_item_id: null,
      }),
    );
    await expect(page.getByTestId('inbox-pane').getByText('Rate-limit login attempts')).toBeVisible();
    await page.getByTestId('inbox-pane').focus();
    // Needs-input sessions are listed first: j moves to the first ticket.
    await page.keyboard.press('j');
    await page.keyboard.press('Enter');
    await expect(page.getByTestId('ticket-detail')).toBeVisible();
  });
});
