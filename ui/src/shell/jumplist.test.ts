import { describe, expect, it } from 'vitest';

import { JUMP_CAP, Jumplist } from './jumplist';

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
});
