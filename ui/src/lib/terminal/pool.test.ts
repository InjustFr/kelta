import { describe, expect, it } from 'vitest';

import { clampCapacity, TerminalViewPool, type PoolView } from './index';

class FakeView implements PoolView {
  mounted: HTMLElement | null = null;
  attaches = 0;
  attached = false;
  disposed = false;
  mounts = 0;
  constructor(readonly id: string) {}
  mount(container: HTMLElement): void {
    this.mounted = container;
    this.mounts += 1;
  }
  unmount(): void {
    this.mounted = null;
  }
  async attach(): Promise<void> {
    if (this.attached) return;
    this.attached = true;
    this.attaches += 1;
  }
  focus(): void {}
  dispose(): void {
    this.disposed = true;
    this.attached = false;
  }
}

function setup(capacity: number) {
  const created: FakeView[] = [];
  const pool = new TerminalViewPool({
    capacity,
    createView: (id) => {
      const v = new FakeView(id);
      created.push(v);
      return v;
    },
  });
  const el = (): HTMLElement => document.createElement('div');
  const viewOf = (id: string): FakeView => created.find((v) => v.id === id && !v.disposed)!;
  return { pool, created, el, viewOf };
}

describe('TerminalViewPool', () => {
  it('creates, mounts and attaches a view on first show', async () => {
    const { pool, el, created } = setup(2);
    const c = el();
    await pool.show('a', c);
    expect(created).toHaveLength(1);
    expect(created[0]!.mounted).toBe(c);
    expect(created[0]!.attaches).toBe(1);
    expect(pool.liveCount).toBe(1);
    expect(pool.visibleCount).toBe(1);
  });

  it('re-shows a pooled view without creating or re-attaching a session', async () => {
    const { pool, el, created } = setup(2);
    await pool.show('a', el());
    pool.hide('a');
    expect(pool.hiddenOrder).toEqual(['a']);
    const c2 = el();
    await pool.show('a', c2);
    expect(created).toHaveLength(1);
    expect(created[0]!.attaches).toBe(1); // attach() is idempotent: no new session_attach
    expect(created[0]!.mounted).toBe(c2);
    expect(pool.hiddenOrder).toEqual([]);
  });

  it('evicts the least recently hidden view beyond capacity', async () => {
    const { pool, el, viewOf } = setup(2);
    for (const id of ['a', 'b', 'c', 'd']) await pool.show(id, el());
    const views = ['a', 'b', 'c', 'd'].map(viewOf);
    // Visible views never count against the capacity.
    pool.evictNow();
    expect(pool.liveCount).toBe(4);
    for (const id of ['a', 'b', 'c', 'd']) pool.hide(id);
    expect(pool.hiddenOrder).toEqual(['a', 'b', 'c', 'd']);
    pool.evictNow();
    expect(pool.hiddenOrder).toEqual(['c', 'd']);
    expect(views.map((v) => v.disposed)).toEqual([true, true, false, false]);
    expect(pool.liveCount).toBe(2);
  });

  it('keeps visible views on top of the capacity', async () => {
    const { pool, el } = setup(1);
    await pool.show('a', el());
    await pool.show('b', el());
    await pool.show('c', el());
    pool.hide('a');
    pool.hide('b');
    pool.evictNow();
    expect(pool.hiddenOrder).toEqual(['b']);
    expect(pool.liveCount).toBe(2); // b (hidden, within capacity) + c (visible)
  });

  it('a re-shown view moves to the end of the eviction order when hidden again', async () => {
    const { pool, el } = setup(2);
    for (const id of ['a', 'b', 'c']) await pool.show(id, el());
    for (const id of ['a', 'b', 'c']) pool.hide(id);
    await pool.show('a', el()); // a leaves the hidden list
    pool.hide('a');
    expect(pool.hiddenOrder).toEqual(['b', 'c', 'a']);
    pool.evictNow();
    expect(pool.hiddenOrder).toEqual(['c', 'a']);
  });

  it('evicts after the current task, so a project switch can show its views first', async () => {
    const { pool, el, created } = setup(2);
    // Project A (s1, s2) was parked, project B (b1, b2) is visible.
    for (const id of ['s1', 's2', 'b1', 'b2']) await pool.show(id, el());
    pool.hide('s1');
    pool.hide('s2');
    pool.evictNow();
    // Switch back to A inside one synchronous flush: B is hidden first, then A is shown.
    pool.hide('b1');
    pool.hide('b2');
    void pool.show('s1', el());
    void pool.show('s2', el());
    await Promise.resolve(); // the queued eviction runs here
    await Promise.resolve();
    expect(created.filter((v) => v.disposed)).toEqual([]);
    expect(pool.hiddenOrder).toEqual(['b1', 'b2']);
    expect(pool.liveCount).toBe(4);
  });

  it('protect() keeps views that are about to be shown', async () => {
    const { pool, el, viewOf } = setup(1);
    for (const id of ['a', 'b', 'c']) await pool.show(id, el());
    const a = viewOf('a');
    for (const id of ['a', 'b', 'c']) pool.hide(id);
    pool.protect(['a']);
    pool.evictNow();
    expect(a.disposed).toBe(false);
    expect(pool.hiddenOrder).toEqual(['a']);
    expect(pool.liveCount).toBe(1);
    // Showing a protected view clears its protection.
    await pool.show('a', el());
    pool.hide('a');
    pool.evictNow();
    expect(pool.hiddenOrder).toEqual(['a']);
  });

  it('touch() refreshes the eviction order', async () => {
    const { pool, el } = setup(3);
    for (const id of ['a', 'b', 'c']) await pool.show(id, el());
    for (const id of ['a', 'b', 'c']) pool.hide(id);
    pool.touch(['a']);
    expect(pool.hiddenOrder).toEqual(['b', 'c', 'a']);
  });

  it('release() disposes a view immediately and forgets it', async () => {
    const { pool, el, viewOf } = setup(2);
    await pool.show('a', el());
    const v = viewOf('a');
    pool.release('a');
    expect(v.disposed).toBe(true);
    expect(pool.liveCount).toBe(0);
    expect(pool.has('a')).toBe(false);
  });

  it('setCapacity() clamps to 1..12 and evicts down to the new capacity', async () => {
    const { pool, el } = setup(4);
    for (const id of ['a', 'b', 'c']) await pool.show(id, el());
    for (const id of ['a', 'b', 'c']) pool.hide(id);
    pool.setCapacity(1);
    pool.evictNow();
    expect(pool.hiddenOrder).toEqual(['c']);
    pool.setCapacity(99);
    expect(pool.capacity).toBe(12);
    pool.setCapacity(0);
    expect(pool.capacity).toBe(1);
    expect(clampCapacity(Number.NaN)).toBe(4);
  });

  it('a view hidden while it is being created is pooled, not mounted', async () => {
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    const view = new FakeView('a');
    const pool = new TerminalViewPool({
      capacity: 2,
      createView: async () => {
        await gate;
        return view;
      },
    });
    const shown = pool.show('a', document.createElement('div'));
    pool.hide('a');
    release();
    await shown;
    expect(view.mounts).toBe(0);
    expect(pool.hiddenOrder).toEqual(['a']);
    expect(pool.liveCount).toBe(1);
  });

  it('shares one creation between concurrent shows of the same session', async () => {
    const { pool, el, created } = setup(2);
    await Promise.all([pool.show('a', el()), pool.show('a', el())]);
    expect(created).toHaveLength(1);
  });
});
