import { afterEach, describe, expect, it } from 'vitest';

import type { ProjectInfo } from '$lib/gen';
import { projects, ui } from '$lib/stores';

import {
  LOOP_SHIPPED,
  loopCurrent,
  loopDone,
  loopShipped,
  offerOnboarding,
  ONBOARDING_DONE,
  rememberShipped,
  type LoopStepId,
} from './firstRun';

const project = (id: string, builtin: boolean) => ({ id, builtin }) as ProjectInfo;

describe('offerOnboarding', () => {
  afterEach(() => {
    localStorage.clear();
    ui.sheets = [];
  });

  it('opens on a fresh install only', () => {
    projects.loaded = true;
    projects.list = [project('home', true)];
    offerOnboarding();
    expect(ui.sheets.map((s) => s.key)).toEqual(['onboarding']);

    ui.sheets = [];
    localStorage.setItem(ONBOARDING_DONE, '1');
    offerOnboarding();
    expect(ui.sheets).toEqual([]);

    localStorage.clear();
    projects.list = [project('home', true), project('shop', false)];
    offerOnboarding();
    expect(ui.sheets).toEqual([]);
  });
});

const w = (over: Partial<Parameters<typeof loopDone>[2][number]> = {}) => ({
  pr_url: null,
  claude_at: null,
  review_due: false,
  claude_replied: false,
  ...over,
});

describe('Your loop', () => {
  const current = (...args: Parameters<typeof loopDone>) => loopCurrent(loopDone(...args));

  it('each step is the current one in turn, from real state', () => {
    const home = [{ builtin: true }];
    const shop = [{ builtin: true }, { builtin: false }];
    const seen: (LoopStepId | null)[] = [
      current(home, null, []),
      current(shop, false, []),
      current(shop, true, []),
      current(shop, true, [w({ claude_at: 'now', review_due: true })]),
      current(shop, true, [w({ claude_at: 'now' })]),
      current(shop, true, [w({ claude_at: 'now', pr_url: 'https://x/pr/1' })]),
    ];
    expect(seen).toEqual(['project', 'source', 'start', 'review', 'ship', null]);
  });

  it('a shipped item is remembered for the next launch', () => {
    expect(loopShipped()).toBe(false);
    rememberShipped();
    expect(localStorage.getItem(LOOP_SHIPPED)).toBe('1');
    expect(loopShipped()).toBe(true);
    localStorage.clear();
  });
});
