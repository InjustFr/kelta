import { describe, expect, it, vi } from 'vitest';

import { GLOBAL, ui } from '$lib/stores';

import {
  backTarget,
  flushJump,
  JUMP_CAP,
  Jumplist,
  jumplist,
  navStep,
  noteFocus,
  SETTLE_MS,
} from './jumplist';

const loc = (n: number) => ({ project: 'p', tab: 't', pane: `pane${n}` });
const view = (v: 'now' | 'reviews') => ({ project: GLOBAL, tab: v, pane: v });

describe('Jumplist', () => {
  it('walks back and forward, and a new jump drops the forward entries', () => {
    const j = new Jumplist();
    j.record(loc(1));
    j.record(loc(2));
    expect(j.record(loc(2))).toBe(false); // same place twice is one entry
    j.record(loc(3));
    expect(j.step(-1)).toEqual(loc(2));
    expect(j.step(-1)).toEqual(loc(1));
    expect(j.step(-1)).toBeNull();
    expect(j.step(1)).toEqual(loc(2));
    // landing on the entry `back` chose records nothing
    expect(j.record(loc(2))).toBe(false);
    j.record(loc(4));
    expect(j.list).toEqual([loc(1), loc(2), loc(4)]);
    expect(j.step(1)).toBeNull();
  });

  it('keeps the last 100 places', () => {
    const j = new Jumplist();
    for (let i = 0; i < JUMP_CAP + 5; i += 1) j.record(loc(i));
    expect(j.list).toHaveLength(JUMP_CAP);
    expect(j.list[0]).toEqual(loc(5));
    expect(j.at).toBe(JUMP_CAP - 1);
  });

  it('records a place once focus rests on it, skipping stopovers, and flushes before a jump', () => {
    vi.useFakeTimers();
    try {
      const before = jumplist.list.length;
      noteFocus(loc(1));
      vi.advanceTimersByTime(SETTLE_MS);
      noteFocus(loc(2)); // the target project's old tab, passed through mid-jump
      noteFocus(loc(3));
      vi.advanceTimersByTime(SETTLE_MS);
      noteFocus(loc(4));
      flushJump(); // Mod+J or nav.back right away still keeps where focus was
      expect(jumplist.list.slice(before)).toEqual([loc(1), loc(3), loc(4)]);
    } finally {
      vi.useRealTimers();
    }
  });

  it('leaves the cursor in place when no live place exists that way', () => {
    jumplist.list = [loc(1), loc(2), loc(3)];
    jumplist.at = 1; // after one nav.back; project 'p' is not open, so every entry is dead
    navStep(1);
    expect(jumplist.at).toBe(1);
    navStep(-1);
    expect(jumplist.at).toBe(1);
  });

  it('a global view is its tab in the GLOBAL layout, and back opens it', () => {
    const j = new Jumplist();
    const now = view('now');
    j.record(now);
    expect(j.record(view('now'))).toBe(false);
    j.record(loc(1));
    expect(j.step(-1)).toEqual(now);

    ui.view = null;
    jumplist.list = [view('reviews'), loc(1)];
    jumplist.at = 1;
    navStep(-1);
    expect([ui.view, jumplist.at]).toEqual(['reviews', 0]);
    ui.view = null;
  });

  it('loads pre-#270 view entries as GLOBAL places', async () => {
    localStorage.setItem(
      'kelta.jumplist.v1',
      JSON.stringify({ list: [{ project: 'shop', tab: '', pane: '', view: 'now' }, loc(1)], at: 1 }),
    );
    vi.resetModules();
    const fresh = await import('./jumplist');
    expect(fresh.jumplist.list).toEqual([view('now'), loc(1)]);
    localStorage.removeItem('kelta.jumplist.v1');
  });

  it('backTarget names where nav.back lands: the last live place before here, without moving', () => {
    jumplist.list = [view('reviews'), loc(1), view('now')];
    jumplist.at = 2;
    // Here is Now (recorded): back skips it and the dead pane, landing on Reviews.
    expect(backTarget(view('now'))).toEqual(view('reviews'));
    // A tool just opened from Now (not recorded yet): back is Now.
    expect(backTarget(loc(9))).toEqual(view('now'));
    expect(jumplist.at).toBe(2);
    jumplist.at = 0;
    expect(backTarget(view('reviews'))).toBeNull();
  });
});
