import { Terminal } from '@xterm/headless';
import { describe, expect, it, vi } from 'vitest';

import { FRAME_DATA, FRAME_EXIT, FRAME_SNAPSHOT } from '$lib/gen/constants';

import { AckBatcher, decodeFrame, FrameHandler } from './frames';
import { FrameScheduler, type FrameDriver } from './raf';

function frame(tag: number, payload: Uint8Array | string): Uint8Array {
  const bytes = typeof payload === 'string' ? new TextEncoder().encode(payload) : payload;
  const out = new Uint8Array(bytes.length + 1);
  out[0] = tag;
  out.set(bytes, 1);
  return out;
}

function exitFrame(code: number): Uint8Array {
  const payload = new Uint8Array(4);
  new DataView(payload.buffer).setInt32(0, code, true);
  return frame(FRAME_EXIT, payload);
}

/** Frame driver that runs when the test says so. */
class ManualDriver implements FrameDriver {
  callbacks: (() => void)[] = [];
  request(cb: () => void): unknown {
    this.callbacks.push(cb);
    return this.callbacks.length - 1;
  }
  cancel(handle: unknown): void {
    this.callbacks[handle as number] = () => {};
  }
  runFrame(): void {
    const cbs = this.callbacks;
    this.callbacks = [];
    for (const cb of cbs) cb();
  }
}

describe('decodeFrame', () => {
  it('decodes data, snapshot and exit frames', () => {
    const data = decodeFrame(frame(FRAME_DATA, 'hi'));
    expect(data.kind).toBe('data');
    expect(data.kind === 'data' && Array.from(data.bytes)).toEqual([104, 105]);
    const snap = decodeFrame(frame(FRAME_SNAPSHOT, 'x'));
    expect(snap.kind).toBe('snapshot');
    expect(snap.kind === 'snapshot' && Array.from(snap.bytes)).toEqual([120]);
    expect(decodeFrame(exitFrame(0))).toEqual({ kind: 'exit', code: 0 });
    expect(decodeFrame(exitFrame(137))).toEqual({ kind: 'exit', code: 137 });
    expect(decodeFrame(exitFrame(-1))).toEqual({ kind: 'exit', code: -1 });
  });

  it('is little-endian and respects the byte offset of a subarray', () => {
    const backing = new Uint8Array(12);
    backing.set(exitFrame(0x01020304), 3);
    expect(decodeFrame(backing.subarray(3, 8))).toEqual({ kind: 'exit', code: 0x01020304 });
  });

  it('survives malformed frames', () => {
    expect(decodeFrame(new Uint8Array(0)).kind).toBe('unknown');
    expect(decodeFrame(new Uint8Array([9, 1, 2])).kind).toBe('unknown');
    expect(decodeFrame(new Uint8Array([FRAME_EXIT, 1])).kind).toBe('exit'); // truncated: code -1
    const empty = decodeFrame(new Uint8Array([FRAME_DATA]));
    expect(empty.kind === 'data' && empty.bytes.length).toBe(0);
  });
});

describe('FrameScheduler', () => {
  it('requests one frame for many tasks and replaces tasks by key', () => {
    const driver = new ManualDriver();
    const s = new FrameScheduler(driver);
    const calls: string[] = [];
    s.schedule('a', () => calls.push('a1'));
    s.schedule('a', () => calls.push('a2'));
    s.schedule('b', () => calls.push('b'));
    expect(driver.callbacks).toHaveLength(1);
    expect(s.armed).toBe(true);
    driver.runFrame();
    expect(calls).toEqual(['a2', 'b']);
    expect(s.armed).toBe(false);
    expect(s.pending).toBe(0);
    // Idle: nothing armed, no callback queued.
    expect(driver.callbacks).toHaveLength(0);
  });

  it('does not loop: tasks queued while running wait for the next frame', () => {
    const driver = new ManualDriver();
    const s = new FrameScheduler(driver);
    let runs = 0;
    s.schedule('a', () => {
      runs += 1;
      if (runs < 3) s.schedule('a', () => runs++);
    });
    driver.runFrame();
    expect(runs).toBe(1);
    expect(driver.callbacks).toHaveLength(1);
    driver.runFrame();
    expect(runs).toBe(2);
    expect(driver.callbacks).toHaveLength(0);
  });

  it('cancel() disarms the frame when nothing is left', () => {
    const driver = new ManualDriver();
    const s = new FrameScheduler(driver);
    s.schedule('a', () => {});
    s.cancel('a');
    expect(s.armed).toBe(false);
    driver.runFrame();
    expect(s.frames).toBe(0);
  });

  it('isolates failing tasks', () => {
    const driver = new ManualDriver();
    const s = new FrameScheduler(driver);
    const err = vi.spyOn(console, 'error').mockImplementation(() => {});
    let ok = false;
    s.schedule('bad', () => {
      throw new Error('boom');
    });
    s.schedule('good', () => (ok = true));
    driver.runFrame();
    expect(ok).toBe(true);
    expect(err).toHaveBeenCalled();
  });
});

describe('AckBatcher', () => {
  it('sums acks and sends one session_ack per frame', () => {
    const driver = new ManualDriver();
    const sent: [number, number][] = [];
    const acks = new AckBatcher((g, n) => sent.push([g, n]), new FrameScheduler(driver));
    acks.setGeneration(7);
    acks.add(100);
    acks.add(50);
    acks.add(1);
    expect(sent).toEqual([]);
    driver.runFrame();
    expect(sent).toEqual([[7, 151]]);
    acks.add(10);
    driver.runFrame();
    expect(sent).toEqual([
      [7, 151],
      [7, 10],
    ]);
    driver.runFrame();
    expect(sent).toHaveLength(2); // nothing pending → no extra ack
  });

  it('holds acks until the generation is known', () => {
    const driver = new ManualDriver();
    const sent: [number, number][] = [];
    const acks = new AckBatcher((g, n) => sent.push([g, n]), new FrameScheduler(driver));
    acks.add(64);
    driver.runFrame();
    expect(sent).toEqual([]);
    expect(acks.pending).toBe(64);
    acks.setGeneration(3);
    driver.runFrame();
    expect(sent).toEqual([[3, 64]]);
  });

  it('drops pending acks on reset (detach) and ignores non-positive values', () => {
    const driver = new ManualDriver();
    const sent: [number, number][] = [];
    const acks = new AckBatcher((g, n) => sent.push([g, n]), new FrameScheduler(driver));
    acks.setGeneration(1);
    acks.add(0);
    acks.add(-5);
    expect(driver.callbacks).toHaveLength(0);
    acks.add(10);
    acks.reset();
    driver.runFrame();
    expect(sent).toEqual([]);
    expect(acks.pending).toBe(0);
  });
});

describe('FrameHandler on a headless terminal', () => {
  function setup() {
    const term = new Terminal({ cols: 20, rows: 4, allowProposedApi: true });
    const driver = new ManualDriver();
    const sent: number[] = [];
    const acks = new AckBatcher((_g, n) => sent.push(n), new FrameScheduler(driver));
    acks.setGeneration(1);
    const exits: number[] = [];
    const handler = new FrameHandler({ term, acks, onExit: (c) => exits.push(c) });
    const barrier = (): Promise<void> => new Promise((resolve) => term.write('', resolve));
    const line = (y: number): string => term.buffer.active.getLine(y)?.translateToString(true) ?? '';
    return { term, driver, sent, handler, exits, barrier, line };
  }

  it('writes data and acks the parsed byte count once per frame', async () => {
    const { handler, barrier, driver, sent, line } = setup();
    handler.handle(frame(FRAME_DATA, 'hello'));
    handler.handle(frame(FRAME_DATA, ' world'));
    await barrier();
    expect(line(0)).toBe('hello world');
    driver.runFrame();
    expect(sent).toEqual([11]);
  });

  it('drops acks for writes that finish after close()', async () => {
    const { barrier, driver, sent, term } = setup();
    const acks = new AckBatcher((_g, n) => sent.push(n), new FrameScheduler(driver));
    const h = new FrameHandler({ term, acks });
    h.handle(frame(FRAME_DATA, 'late'));
    h.close();
    acks.reset();
    await barrier();
    acks.setGeneration(2);
    driver.runFrame();
    expect(sent).toEqual([]);
  });

  it('snapshot resets the screen before painting and acks its bytes', async () => {
    const { handler, barrier, driver, sent, line } = setup();
    handler.handle(frame(FRAME_DATA, 'stale text'));
    handler.handle(frame(FRAME_SNAPSHOT, '\x1b[1;1HFRESH'));
    await barrier();
    expect(line(0)).toBe('FRESH');
    driver.runFrame();
    expect(sent).toEqual([
      new TextEncoder().encode('stale text').length + new TextEncoder().encode('\x1b[1;1HFRESH').length,
    ]);
  });

  it('reports the exit code and ignores frames after close()', async () => {
    const { handler, barrier, exits, line, sent, driver } = setup();
    handler.handle(exitFrame(3));
    expect(exits).toEqual([3]);
    handler.close();
    handler.handle(frame(FRAME_DATA, 'late'));
    handler.handle(exitFrame(4));
    await barrier();
    expect(line(0)).toBe('');
    expect(exits).toEqual([3]);
    driver.runFrame();
    expect(sent).toEqual([]);
  });

  it('ignores unknown tags', async () => {
    const { handler, barrier, line } = setup();
    handler.handle(new Uint8Array([0x7f, 65, 66]));
    await barrier();
    expect(line(0)).toBe('');
  });
});
