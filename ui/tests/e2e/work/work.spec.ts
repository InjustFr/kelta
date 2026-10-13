// L9 e2e on the IPC mock (VITE_IPC=mock): start work from the board, move a ticket (and roll back),
// approve a review, Now's review requests and ticket rows. The shell is L2's: the harness page hosts the panes.
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
  await dispatch(page, 'tickets.board');
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
    await page.keyboard.press('Enter'); // the first move (In progress) is lit
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
    // The mock's unfinished work is listed first (#354); Enter starts anyway.
    await expect(sheet.getByTestId('finish-first')).toContainText('SHOP-142');
    await page.keyboard.press('Enter');
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
    await dispatch(page, 'reviews.open', { project_id: 'shop' });
    await expect(page.locator('.row .ttl', { hasText: 'SHOP-150: cache product images' })).toBeVisible();
    await page.keyboard.press('Enter'); // focuses the first review's detail, in the column beside the list
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

test.describe('reviewing others', () => {
  test('request row, Enter, detail, s, Ctrl+Enter starts a local review', async ({ page }) => {
    const errors = await boot(page);
    await dispatch(page, 'reviews.open', { project_id: 'shop' });
    await expect(page.locator('.row .ttl', { hasText: 'SHOP-150: cache product images' })).toBeVisible();
    await page.keyboard.press('Enter'); // first request: SHOP-150, its detail column takes the keys
    const detail = page.getByTestId('review-detail');
    await expect(detail).toBeVisible();
    await page.keyboard.press('s');
    const sheet = page.getByRole('dialog', { name: /acme\/shop-api#311|pr-311/ });
    await expect(sheet).toBeVisible();
    await page.keyboard.press('Control+Enter');
    await expect(sheet).toBeHidden();
    const started = await page.evaluate(() =>
      window.__kelta!.stores.work.all.some((w) => w.kind === 'review' && w.review?.number === 311),
    );
    expect(started).toBe(true);
    expect(errors).toEqual([]);
  });

  test('pending comments are submitted with the decision', async ({ page }) => {
    const errors = await boot(page);
    await page.evaluate(() =>
      window.__kelta!.stores.layout.open('shop', {
        content: {
          kind: 'review_detail',
          review: { account: 'github-acme', repo: 'acme/shop-web', number: 101 },
        },
        placement: 'new_tab',
        focus: true,
        tab_title: 'PR',
        work_item_id: null,
      }),
    );
    const detail = page.getByTestId('review-detail');
    await expect(detail.getByTestId('pending-comments')).toContainText('2 pending comments');
    await page.keyboard.press('a');
    await expect(page.getByTestId('toasts')).toContainText('Approved');
    await expect(detail.getByTestId('pending-comments')).toBeHidden();
    expect(errors).toEqual([]);
  });
});

test.describe('now', () => {
  async function openNow(page: Page): Promise<void> {
    await page.evaluate(() =>
      window.__kelta!.stores.layout.open('shop', {
        content: { kind: 'inbox' },
        placement: 'new_tab',
        focus: true,
        tab_title: 'Now',
        work_item_id: null,
      }),
    );
    await expect(page.getByTestId('inbox-pane')).toBeVisible();
  }

  test('lists review requests, also on repos bound to no project', async ({ page }) => {
    const errors = await boot(page);
    await openNow(page);
    const now = page.getByTestId('inbox-pane');
    await expect(now.locator('.row[data-row^="r:"]').first()).toBeVisible();
    await expect(now.getByText('Terraform: add read replica')).toBeVisible();
    expect(errors).toEqual([]);
  });

  test('g on an Up next ticket opens its detail', async ({ page }) => {
    await boot(page);
    await openNow(page);
    const now = page.getByTestId('inbox-pane');
    // The list is virtual: the filter bar leaves room for fewer rows, so j scrolls the ticket into view.
    await expect(now.locator('.row').first()).toBeVisible();
    await now.focus();
    for (let i = 0; i < 30; i += 1) {
      const id = await now.locator('.row[aria-current="true"]').getAttribute('data-row');
      if (id === 't:jira-acme:SHOP-151') break;
      await page.keyboard.press('j');
    }
    await page.keyboard.press('g');
    await expect(page.getByTestId('ticket-detail')).toBeVisible();
  });
});

test.describe('tickets and reviews side by side', () => {
  test('one action opens my tickets beside Reviews', async ({ page }) => {
    const errors = await boot(page);
    await dispatch(page, 'tickets.overview');
    await expect(page.getByTestId('tickets-pane')).toBeVisible();
    await expect(
      page.getByTestId('reviews-pane').getByRole('button', { name: /^Asked of you/ }),
    ).toBeVisible();
    expect(errors).toEqual([]);
  });
});
