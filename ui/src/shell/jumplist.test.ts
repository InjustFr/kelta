import { describe, expect, it, vi } from 'vitest';

import { flushJump, JUMP_CAP, Jumplist, jumplist, navStep, noteFocus, SETTLE_MS } from './jumplist';

const loc = (n: number) => ({ project: 'p', tab: 't', pane: `pane${n}` });

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
});
