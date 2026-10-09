import { Terminal } from '@xterm/headless';
import { describe, expect, it } from 'vitest';

import {
  SWALLOWED_CSI,
  SWALLOWED_OSC,
  type CsiQuery,
  type OscQuery,
} from '$lib/gen/terminal_queries';

import { csiMatches, installQueryHandlers, isOscQuery } from './queries';

const ESC = '\x1b';

/** A concrete query sequence for each generated entry. */
function csiSamples(q: CsiQuery): string[] {
  const params = q.params.length > 0 ? q.params.map(String) : [''];
  return params.map((p) => `${ESC}[${q.prefix ?? ''}${p}${q.intermediates ?? ''}${q.final}`);
}

function oscSample(q: OscQuery): string {
  return q.ident === 4 ? `${ESC}]4;1;?${ESC}\\` : `${ESC}]${q.ident};?${ESC}\\`;
}

async function feed(term: Terminal, data: string): Promise<void> {
  await new Promise<void>((resolve) => term.write(data, resolve));
}

function make(install: boolean): { term: Terminal; replies: string[] } {
  const term = new Terminal({ cols: 40, rows: 5, allowProposedApi: true });
  const replies: string[] = [];
  term.onData((d) => replies.push(d));
  if (install) installQueryHandlers(term.parser);
  return { term, replies };
}

describe('query swallowing', () => {
  it('xterm answers these queries by itself without the handlers (the test is meaningful)', async () => {
    const { term, replies } = make(false);
    for (const q of SWALLOWED_CSI) for (const seq of csiSamples(q)) await feed(term, seq);
    expect(replies.length).toBeGreaterThan(0);
  });

  it('swallows every CSI query of the generated list', async () => {
    for (const q of SWALLOWED_CSI) {
      for (const seq of csiSamples(q)) {
        const { term, replies } = make(true);
        await feed(term, seq);
        expect(replies, `${q.name} ${JSON.stringify(seq)}`).toEqual([]);
      }
    }
  });

  it('swallows every OSC query of the generated list', async () => {
    for (const q of SWALLOWED_OSC) {
      const { term, replies } = make(true);
      await feed(term, oscSample(q));
      expect(replies, q.name).toEqual([]);
    }
  });

  it('swallows exactly the listed queries: other sequences keep their behaviour', async () => {
    // Un-listed DSR parameter: handled by xterm (cursor position report is DSR 6, listed; DSR ?6 is not).
    const without = make(false);
    const withHandlers = make(true);
    const sequences = [`${ESC}[?6n`, `${ESC}[?996n`, `${ESC}[?1;2c`, `${ESC}[=c`, `${ESC}[1$z`];
    for (const seq of sequences) {
      await feed(without.term, seq);
      await feed(withHandlers.term, seq);
    }
    expect(withHandlers.replies).toEqual(without.replies);
  });

  it('does not swallow colour changes or other OSCs', async () => {
    const term = new Terminal({ cols: 20, rows: 3, allowProposedApi: true });
    const seen: string[] = [];
    // Registered first = consulted last: sees only what the swallowing handlers decline.
    for (const ident of [4, 10, 11, 12, 0, 8]) {
      term.parser.registerOscHandler(ident, (data) => {
        seen.push(`${ident};${data}`);
        return true;
      });
    }
    installQueryHandlers(term.parser);
    await feed(term, `${ESC}]4;1;rgb:ff/00/00${ESC}\\`);
    await feed(term, `${ESC}]11;#112233${ESC}\\`);
    await feed(term, `${ESC}]0;title${ESC}\\`);
    await feed(term, `${ESC}]8;;https://example.com${ESC}\\`);
    await feed(term, `${ESC}]10;?${ESC}\\`); // query: swallowed, never reaches the probe
    expect(seen).toEqual(['4;1;rgb:ff/00/00', '11;#112233', '0;title', '8;;https://example.com']);
  });

  it('keeps swallowing after a full reset (snapshots start with RIS)', async () => {
    const { term, replies } = make(true);
    await feed(term, `${ESC}c`);
    await feed(term, `${ESC}[c${ESC}[>c${ESC}[6n`);
    expect(replies).toEqual([]);
  });

  it('classifies params and OSC payloads', () => {
    const dsr = SWALLOWED_CSI.find((q) => q.name === 'DSR')!;
    expect(csiMatches(dsr, [5])).toBe(true);
    expect(csiMatches(dsr, [6])).toBe(true);
    expect(csiMatches(dsr, [0])).toBe(false);
    expect(csiMatches(dsr, [[6]])).toBe(true);
    expect(csiMatches(dsr, [])).toBe(false);
    const osc4 = SWALLOWED_OSC.find((q) => q.ident === 4)!;
    expect(isOscQuery(osc4, '1;?')).toBe(true);
    expect(isOscQuery(osc4, '1;rgb:00/00/00')).toBe(false);
    expect(isOscQuery(osc4, '1;rgb:00/00/00;2;?')).toBe(true);
    const osc11 = SWALLOWED_OSC.find((q) => q.ident === 11)!;
    expect(isOscQuery(osc11, '?')).toBe(true);
    expect(isOscQuery(osc11, '#fff')).toBe(false);
  });
});
