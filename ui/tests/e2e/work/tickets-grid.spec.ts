// Tickets list grid (#251) on the IPC mock: shared column tracks, room for the title, one baseline,
// a toolbar of at most six controls, and the order columns drop in as the list narrows.
import { expect, test, type Page } from '@playwright/test';

import { boot } from '../shell/helpers';

const LONG = 'Invoice rounding differs from the ledger on multiline orders'; // 60 characters

/** 20 extra Mine tickets with mixed optional data: key length, PR, waiting, sprint, age, title. */
async function seed(page: Page): Promise<void> {
  await page.evaluate((long) => {
    const mock = window.__keltaMock!;
    const base = mock.state.tickets.find((i) => i.ticket.ref.key === 'SHOP-142')!;
    const day = 86_400_000;
    for (let n = 0; n < 20; n++) {
      const item = structuredClone(base);
      const key = `SHOP-${n % 3 === 0 ? 7 + n : 9000 + n * 111}`;
      item.ticket.ref = { ...item.ticket.ref, key, id: `x${n}` };
      item.ticket.title =
        n === 1 ? long : `Seeded ticket ${n}${n % 4 === 0 ? ' with a longer title to clip' : ''}`;
      item.ticket.status_since = new Date(Date.now() - (n % 5) * 6 * day).toISOString();
      item.ticket.sprint = n % 2 ? item.ticket.sprint : null;
      item.work_item_id = null;
      item.prs = n % 3 === 1 ? item.prs.slice(0, 1) : [];
      item.waiting =
        n % 4 === 2
          ? [
              {
                person: 'gh:anna',
                name: 'Anna',
                since: new Date().toISOString(),
                matched: true,
                why: 'review',
              },
            ]
          : [];
      mock.state.tickets.push(item);
    }
  }, LONG);
}

async function openRailTickets(page: Page): Promise<void> {
  await boot(page);
  await seed(page);
  await page.getByTestId('rail-tickets').click();
  await page.getByTestId('tickets-pane').press('Shift+R'); // past the lists loaded at boot
  await expect(page.locator('[data-key="jira-acme:SHOP-7"]')).toBeVisible();
}

/** Ticket rows on screen (VirtualList renders the visible ones). */
const rows = (page: Page) => page.getByTestId('tickets-pane').locator('.k-row[data-key]');

test.use({ viewport: { width: 1440, height: 900 } });

test('rows share one grid: key, title, status, age and source start at one x on every row', async ({
  page,
}) => {
  await openRailTickets(page);
  await page.setViewportSize({ width: 1600, height: 900 }); // detail open, list past 720px
  await page.getByRole('button', { name: /^View/ }).click();
  await page.getByLabel('Group (g)').selectOption('none');
  await page.keyboard.press('Escape');
  const lefts = await rows(page).evaluateAll((els) =>
    els.map((row) =>
      ['.c-key', '.c-title', '[data-status]', '.c-age', '.c-source'].map(
        (sel) => row.querySelector(sel)!.getBoundingClientRect().left,
      ),
    ),
  );
  expect(lefts.length).toBeGreaterThanOrEqual(20);
  for (let col = 0; col < 5; col++) expect(new Set(lefts.map((l) => l[col])).size).toBe(1);
  expect(await rows(page).locator('.c-age').first().isVisible()).toBe(true);
  expect(await rows(page).locator('.c-source').first().isVisible()).toBe(true);
});

test('the title takes the free width: its cell keeps a fixed share of the row with the detail open', async ({
  page,
}) => {
  await openRailTickets(page);
  await expect(page.getByTestId('ticket-detail')).toBeVisible();
  const row = page.locator('[data-key="jira-acme:SHOP-9111"]');
  await expect(row).not.toHaveAttribute('aria-current', 'true');
  await expect(row.locator('.c-title .text')).toHaveText(LONG);
  // Layout, not glyphs: the tracks are sized in px around one 1fr title, so the share holds whatever the
  // runner's system-ui font. At #251's 1440px the share is 0.44: the ~295px cell that showed 45 of the 60
  // characters with macOS's font (42 with the ubuntu runner's, #256).
  const share = await row.evaluate((r) => r.querySelector('.c-title .text')!.clientWidth / r.clientWidth);
  expect(share).toBeGreaterThanOrEqual(0.43);
});

for (const scheme of ['light', 'dark'] as const) {
  test.describe(`${scheme} theme`, () => {
    test.use({ colorScheme: scheme });

    test('text cells share one baseline; lamps and status dots are centred on the row', async ({
      page,
    }, info) => {
      await openRailTickets(page);
      const row = page.locator('[data-key="jira-acme:SHOP-142"]'); // work lamp, PR lamp, aged, source
      await expect(row.locator('.c-lamp [data-attention]')).toBeVisible();
      const m = await row.evaluate((r) => {
        const base = (el: Element): number => {
          const probe = document.createElement('span');
          probe.style.cssText = 'display:inline-block;width:0;height:0';
          el.prepend(probe);
          const y = probe.getBoundingClientRect().bottom;
          probe.remove();
          return y;
        };
        const mid = (el: Element): number => {
          const b = el.getBoundingClientRect();
          return b.top + b.height / 2;
        };
        const rb = r.getBoundingClientRect();
        return {
          row: rb.top + rb.height / 2,
          baselines: ['.c-key', '.c-title .text', '[data-status] .chip'].map((s) =>
            base(r.querySelector(s)!),
          ),
          marks: ['.c-lamp [data-attention]', '[data-pr] [data-attention]', '[data-status] .dot'].map((s) =>
            mid(r.querySelector(s)!),
          ),
        };
      });
      for (const b of m.baselines) expect(Math.abs(b - m.baselines[0]!)).toBeLessThanOrEqual(0.5);
      for (const y of m.marks) expect(Math.abs(y - m.row)).toBeLessThanOrEqual(1.5);
      await info.attach(`tickets-${scheme}`, {
        body: await page.getByTestId('tickets-pane').screenshot(),
        contentType: 'image/png',
      });
    });
  });
}

test('the toolbar shows at most six controls; person, sprint, group and sort sit in View', async ({
  page,
}) => {
  await openRailTickets(page);
  const bar = page.getByTestId('tickets-pane').locator('header.k-toolbar');
  const controls = await bar.evaluate(
    (h) =>
      [...h.children].filter(
        (el) =>
          el.matches('button, input, select, [role="tablist"]') || el.querySelector('button, input, select'),
      ).length,
  );
  expect(controls).toBeLessThanOrEqual(6);
  await expect(bar.getByLabel('Sort')).toHaveCount(0);
  await bar.getByRole('button', { name: /^View/ }).click();
  const view = bar.getByRole('dialog', { name: 'View' });
  for (const label of ['Person', 'Group (g)', 'Sort']) await expect(view.getByLabel(label)).toBeVisible();
  await expect(view.getByRole('button', { name: /Current sprint/ })).toBeVisible();
});

test('narrow lists drop source first, then the age, then the signals; key and title stay', async ({
  page,
}) => {
  await openRailTickets(page);
  const row = page.locator('[data-key="jira-acme:SHOP-151"]'); // aged, from one of several sources
  const shown = async () =>
    Object.fromEntries(
      await Promise.all(
        ['.c-key', '.c-title', '.c-signals', '[data-status]', '.c-age', '.c-source'].map(
          async (s) => [s, await row.locator(s).isVisible()] as const,
        ),
      ),
    );
  const listWidth = () =>
    page
      .getByTestId('tickets-pane')
      .locator('.list')
      .evaluate((l) => l.clientWidth);

  // default: detail open at 1440, list about 670px: everything but the source
  expect(await listWidth()).toBeLessThan(720);
  expect(await shown()).toMatchObject({ '.c-age': true, '.c-signals': true, '.c-source': false });

  await page.setViewportSize({ width: 1600, height: 900 });
  expect(await listWidth()).toBeGreaterThanOrEqual(720);
  expect(Object.values(await shown()).every(Boolean)).toBe(true);

  await page.setViewportSize({ width: 1200, height: 900 });
  expect(await listWidth()).toBeLessThan(720);
  expect(await shown()).toMatchObject({
    '.c-key': true,
    '.c-title': true,
    '.c-signals': true,
    '[data-status]': true,
    '.c-age': true,
    '.c-source': false,
  });

  await page.setViewportSize({ width: 850, height: 900 });
  expect(await listWidth()).toBeLessThan(480);
  expect(await shown()).toMatchObject({ '.c-title': true, '.c-age': false, '.c-signals': true });

  await page.setViewportSize({ width: 800, height: 900 });
  expect(await listWidth()).toBeLessThan(420);
  expect(await shown()).toMatchObject({ '.c-key': true, '.c-title': true, '.c-signals': false });
});
