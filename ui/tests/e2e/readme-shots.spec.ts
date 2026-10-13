// README screenshots (docs/images/readme/*.webp) on the mock IPC: 1440x900, dark, macOS chords, mock
// fixtures only. Needs cwebp (`brew install webp`). Skipped unless README_SHOTS=1: `cd ui && README_SHOTS=1 pnpm exec playwright test readme-shots`.
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

import { expect, test, type Page } from '@playwright/test';

import { boot, callsOf, SESSIONS } from './shell/helpers';

test.skip(!process.env.README_SHOTS, 'set README_SHOTS=1 to recapture the README screenshots');
test.use({ viewport: { width: 1440, height: 900 }, colorScheme: 'dark' });

const ESC = '\x1b[';
const [dim, bold, orange, green, blue, reset] = ['90m', '1m', '38;5;209m', '32m', '34m', '0m'].map(
  (c) => ESC + c,
);

const CLAUDE = [
  `${orange}╭──────────────────────────────────────────────╮${reset}`,
  `${orange}│${reset} ${orange}✻${reset} ${bold}Welcome to Claude Code${reset}                     ${orange}│${reset}`,
  `${orange}│${reset}   ${dim}cwd: ~/code/shop-api.worktrees/SHOP-142${reset}    ${orange}│${reset}`,
  `${orange}╰──────────────────────────────────────────────╯${reset}`,
  '',
  `> Work on SHOP-142: Rate-limit login attempts.`,
  `  ${dim}Acceptance: 5 failed logins per IP in 15 min lock the IP${reset}`,
  `  ${dim}for 15 min; respond 429 with Retry-After.${reset}`,
  '',
  `● I will put a sliding-window limiter in front of the login`,
  `  handler, keyed by client IP, and cover it with tests.`,
  '',
  `${green}●${reset} ${bold}Read${reset}(src/auth/login.rs)`,
  `  ${dim}⎿  Read 84 lines${reset}`,
  '',
  `${green}●${reset} ${bold}Write${reset}(src/auth/rate_limit.rs)`,
  `  ${dim}⎿  Wrote 46 lines to src/auth/rate_limit.rs${reset}`,
  '',
  `${green}●${reset} ${bold}Update${reset}(src/auth/login.rs)`,
  `  ${dim}⎿  Updated src/auth/login.rs with 12 additions and 2 removals${reset}`,
  '',
  `${green}●${reset} ${bold}Bash${reset}(cargo test -p auth login::)`,
  `  ${dim}⎿  test result: ok. 9 passed; 0 failed; finished in 0.41s${reset}`,
  '',
  `${orange}✻ Adding the Retry-After test…${reset} ${dim}(esc to interrupt)${reset}`,
  '',
  `${dim}${'─'.repeat(64)}${reset}`,
  '> ',
].join('\r\n');

const RUST = `use std::time::{Duration, Instant};
use dashmap::DashMap;

/// Failed logins per client IP in a sliding 15-minute window.
pub struct LoginLimiter {
    window: Duration,
    max_failures: usize,
    failures: DashMap<IpAddr, Vec<Instant>>,
}

impl LoginLimiter {
    pub fn check(&self, ip: IpAddr) -> Result<(), Duration> {
        let now = Instant::now();
        let mut hits = self.failures.entry(ip).or_default();
        hits.retain(|t| now - *t < self.window);
        if hits.len() >= self.max_failures {
            return Err(self.window - (now - hits[0]));
        }
        Ok(())
    }

    pub fn record_failure(&self, ip: IpAddr) {
        self.failures.entry(ip).or_default().push(Instant::now());
    }
}`;

/** A minimal Rust highlighter for the nvim scene: comments, keywords, types, calls. */
function highlight(line: string): string {
  if (line.trimStart().startsWith('//')) return dim + line + reset;
  return line
    .replace(/\b(use|pub|struct|impl|fn|let|mut|if|return|self)\b/g, `${ESC}35m$1${reset}`)
    .replace(/\b([A-Z]\w*)\b/g, `${ESC}33m$1${reset}`)
    .replace(/\b([a-z_]\w*)(?=\()/g, `${blue}$1${reset}`);
}

/** Paints Claude Code and nvim into the SHOP-142 panes (mock IPC Data frames). */
async function paintTerminals(page: Page): Promise<void> {
  const rows = await page.evaluate(
    (id) => window.__keltaMock!.state.sessions.find((s) => s.id === id)!.rows,
    SESSIONS.shopNvim,
  );
  const code = RUST.split('\n').map((l, i) => `${dim}${String(i + 1).padStart(3)}${reset} ${highlight(l)}`);
  const tildes = Array.from({ length: Math.max(0, rows - code.length - 2) }, () => `${blue}~${reset}`);
  const status = `${ESC}30;42m NORMAL ${reset}${ESC}32m  feat/SHOP-142 ${reset} src/auth/rate_limit.rs`;
  const nvim = [
    ...code,
    ...tildes,
    status,
    `${dim}"src/auth/rate_limit.rs" ${code.length}L, 812B written${reset}`,
  ];
  await page.evaluate(
    ([c, cs, n, ns]) => {
      window.__keltaMock!.output(c!, `\x1b[2J\x1b[H${cs}`);
      window.__keltaMock!.output(n!, `\x1b[2J\x1b[H${ns}`);
    },
    [SESSIONS.shopClaude, CLAUDE, SESSIONS.shopNvim, nvim.join('\r\n')],
  );
}

/** Screenshots the page into docs/images/readme/<name>.webp (lossless, smallest: cwebp -z 9). */
async function shoot(page: Page, name: string): Promise<void> {
  await page.mouse.move(0, 899);
  const png = test.info().outputPath(`${name}.png`);
  await page.screenshot({ path: png, animations: 'disabled', caret: 'hide' });
  const webp = fileURLToPath(new URL(`../../../docs/images/readme/${name}.webp`, import.meta.url));
  execFileSync('cwebp', ['-quiet', '-lossless', '-z', '9', '-metadata', 'none', png, '-o', webp]);
}

test('work-item: SHOP-142 with Claude and nvim side by side, under the desk header', async ({ page }) => {
  await boot(page, 'macos');
  await expect(page.getByTestId('work-header')).toBeVisible();
  const acks = (await callsOf(page, 'session_ack')).length;
  await paintTerminals(page);
  // The macOS renderer draws on a canvas: wait for both panes to ack the painted frames.
  await expect.poll(async () => (await callsOf(page, 'session_ack')).length).toBeGreaterThanOrEqual(acks + 2);
  await shoot(page, 'work-item');
});

test('tickets: the Tickets view grouped by flow, with the ticket detail', async ({ page }) => {
  await boot(page, 'macos');
  await page.getByTestId('rail-tickets').click();
  const host = page.getByTestId('global-host');
  await host.locator('[data-key="redmine-corp:4567"] .k-row-key').click();
  await expect(host.getByTestId('ticket-detail')).toBeVisible();
  await shoot(page, 'tickets');
});

test('reviews: the Pull requests view with a pull request detail', async ({ page }) => {
  await boot(page, 'macos');
  await page.getByTestId('rail-reviews').click();
  const host = page.getByTestId('global-host');
  await host.locator('.row .ttl', { hasText: 'SHOP-150: cache product images' }).click();
  await expect(
    host
      .getByTestId('review-brief')
      .getByRole('heading', { level: 3, name: 'SHOP-150: cache product images' }),
  ).toBeVisible();
  await shoot(page, 'reviews');
});
