// Performance contract (ARCH §13): no timers, frame callbacks or animations at idle.
import { expect, test } from '@playwright/test';

import { boot } from './helpers';

test('an idle shell arms no intervals, timeouts or animation frames', async ({ page }) => {
  await page.addInitScript(() => {
    const w = window as unknown as { __timers: Record<string, number> };
    w.__timers = { setInterval: 0, setTimeout: 0, requestAnimationFrame: 0 };
    const wrap = <K extends 'setInterval' | 'setTimeout' | 'requestAnimationFrame'>(name: K): void => {
      const original = window[name] as (...args: unknown[]) => number;
      (window as unknown as Record<string, unknown>)[name] = (...args: unknown[]): number => {
        w.__timers[name] = (w.__timers[name] ?? 0) + 1;
        return original.apply(window, args);
      };
    };
    wrap('setInterval');
    wrap('setTimeout');
    wrap('requestAnimationFrame');
  });
  await boot(page);
  // Let startup work (first paint mark, layout loads, toasts) finish, then watch an idle window.
  await page.waitForTimeout(1500);
  await page.evaluate(() => {
    const w = window as unknown as { __timers: Record<string, number> };
    for (const k of Object.keys(w.__timers)) w.__timers[k] = 0;
  });
  await page.waitForTimeout(2500);
  const counts = await page.evaluate(
    () => (window as unknown as { __timers: Record<string, number> }).__timers,
  );
  expect(counts).toEqual({ setInterval: 0, setTimeout: 0, requestAnimationFrame: 0 });
  // No running CSS animations either.
  const animations = await page.evaluate(() => document.getAnimations().length);
  expect(animations).toBe(0);
});
