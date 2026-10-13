// Tickets list grid (#251, #400) on the IPC mock: shared column tracks, room for the title, the stage
// track centred on the row, a toolbar of at most six controls, and what drops as the list narrows.
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

test('rows share one grid: key, title, stage track, age and source start at one x on every row', async ({
  page,
}) => {
  await openRailTickets(page);
  await page.setViewportSize({ width: 1600, height: 900 }); // detail open, list past 720px
  await page.getByRole('button', { name: /^View/ }).click();
  await page.getByLabel('Group (Shift+G)').selectOption('none');
  await page.keyboard.press('Escape');
  const lefts = await rows(page).evaluateAll((els) =>
    els.map((row) =>
      ['.c-key', '.c-what', '.c-stage', '.c-age', '.c-source'].map(
        (sel) => row.querySelector(sel)!.getBoundingClientRect().left,
      ),
    ),
  );
  expect(lefts.length).toBeGreaterThanOrEqual(10); // two-line rows (#400): fewer on screen
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
  await expect(row.locator('.c-what .text')).toHaveText(LONG);
  // Layout, not glyphs: the tracks are sized in px around one 1fr title, so the share holds whatever the
  // runner's system-ui font. At #251's 1440px the share is 0.44 with the 240px sidebar, 0.416 with #362's 288px default: the ~295px cell that showed 45 of the 60
  // characters with macOS's font (42 with the ubuntu runner's, #256).
  const share = await row.evaluate((r) => r.querySelector('.c-what .text')!.clientWidth / r.clientWidth);
  expect(share).toBeGreaterThanOrEqual(0.41);
});

for (const scheme of ['light', 'dark'] as const) {
  test.describe(`${scheme} theme`, () => {
    test.use({ colorScheme: scheme });

    test('key, stage mark and age are centred on the row (#400)', async ({ page }, info) => {
      await openRailTickets(page);
      const row = page.locator('[data-key="jira-acme:SHOP-142"]'); // work, PR, two-line title
      await expect(row.locator('[data-mark]')).toBeVisible();
      const m = await row.evaluate((r) => {
        const mid = (el: Element): number => {
          const b = el.getBoundingClientRect();
          return b.top + b.height / 2;
        };
        const rb = r.getBoundingClientRect();
        return {
          row: rb.top + rb.height / 2,
          marks: ['.c-key', '.c-stage .mark', '.c-age'].map((s) => mid(r.querySelector(s)!)),
        };
      });
      for (const y of m.marks) expect(Math.abs(y - m.row)).toBeLessThanOrEqual(1.5);
      await info.attach(`tickets-${scheme}`, {
        body: await page.getByTestId('tickets-pane').screenshot(),
        contentType: 'image/png',
      });
    });

    test('keys take --k-info, titles --k-fg-strong at 500 and 600 on the selected row (#382)', async ({
      page,
    }) => {
      await openRailTickets(page);
      const s = await rows(page).evaluateAll((els) => {
        const token = (name: string): string => {
          const probe = document.createElement('span');
          probe.style.color = `var(${name})`;
          document.body.append(probe);
          const c = getComputedStyle(probe).color;
          probe.remove();
          return c;
        };
        const title = (r: Element | undefined) => getComputedStyle(r!.querySelector('.c-what .text')!);
        return {
          info: token('--k-info'),
          fg: token('--k-fg'),
          strong: token('--k-fg-strong'),
          keys: [...new Set(els.map((r) => getComputedStyle(r.querySelector('.c-key')!).color))],
          titleColor: title(els[0]).color,
          selected: title(els.find((r) => r.classList.contains('selected'))).fontWeight,
          other: title(els.find((r) => !r.classList.contains('selected'))).fontWeight,
        };
      });
      expect(s.keys).toEqual([s.info]);
      expect(s.strong).not.toBe(s.fg);
      expect(s.titleColor).toBe(s.strong);
      expect([s.other, s.selected]).toEqual(['500', '600']);
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
  for (const label of ['Person', 'Group (Shift+G)', 'Sort'])
    await expect(view.getByLabel(label)).toBeVisible();
  await expect(view.getByRole('button', { name: /Current sprint/ })).toBeVisible();
});

test('narrow lists drop the source first, then put the stage track under the title; key, title and age stay', async ({
  page,
}) => {
  await openRailTickets(page);
  const row = rows(page).first(); // from one of several sources
  const listWidth = () =>
    page
      .getByTestId('tickets-pane')
      .locator('.list')
      .evaluate((l) => l.clientWidth);
  for (const width of [1600, 1440, 1200, 850]) {
    await page.setViewportSize({ width, height: 900 });
    const list = await listWidth();
    for (const s of ['.c-key', '.c-what', '.c-stage', '.c-age'])
      await expect(row.locator(s), `${s} at ${width}px`).toBeVisible();
    await expect(row.locator('.c-source'), `source at ${width}px`).toBeVisible({ visible: list >= 720 });
    const under = await row.evaluate((r) => {
      const what = r.querySelector('.c-what')!.getBoundingClientRect();
      return r.querySelector('.c-stage')!.getBoundingClientRect().top >= what.bottom - 1;
    });
    expect(under, `track under the title at ${width}px (list ${list}px)`).toBe(list < 560);
  }
});
