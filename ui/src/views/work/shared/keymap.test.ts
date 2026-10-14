import { describe, expect, it } from 'vitest';

import { VERB_OF, queueKeys, reviewsKeys, ticketsKeys, type ListKey } from './keymap';

const LISTS: Record<string, ListKey[]> = {
  queue: queueKeys(),
  tickets: ticketsKeys('Start work'),
  reviews: reviewsKeys(),
};

/** A hint's keys: its alternatives, or a `then` sequence as one key. */
const keysOf = (keys: string): string[] => (keys.includes(' then ') ? [keys] : keys.split(' '));

describe('one verb per letter (#417)', () => {
  it('each shared key means the same verb in every list, and each list binds a key once', () => {
    const wrong: string[] = [];
    for (const [list, entries] of Object.entries(LISTS)) {
      const seen = new Set<string>();
      for (const [keys, label, verb] of entries) {
        for (const key of keysOf(keys)) {
          if (seen.has(key)) wrong.push(`${list}: ${key} bound twice`);
          seen.add(key);
          if (key in VERB_OF && VERB_OF[key] !== verb) wrong.push(`${list}: ${key} = ${label}`);
        }
        // A verb names a shared key: `c` tagged "comment" is fine, `p` tagged "comment" is not.
        if (verb && !keysOf(keys).some((k) => VERB_OF[k] === verb))
          wrong.push(`${list}: ${keys} is not ${verb}`);
      }
    }
    expect(wrong).toEqual([]);
  });

  it('b marks in every list that starts tickets, Space is the side panel in every list (#440)', () => {
    const verbOf = (list: string, key: string) =>
      LISTS[list]!.find(([keys]) => keysOf(keys).includes(key))?.[2];
    for (const list of ['queue', 'tickets']) expect(verbOf(list, 'b')).toBe('mark');
    for (const list of Object.keys(LISTS)) expect(verbOf(list, 'space')).toBe('panel');
  });
});
