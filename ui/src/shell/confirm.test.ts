import { describe, expect, it } from 'vitest';

import { ConfirmStore, PromptStore } from './confirm.svelte';

describe('ConfirmStore', () => {
  it('resolves with the chosen action id, or null when dismissed', async () => {
    const store = new ConfirmStore();
    const a = store.ask({ title: 'A', body: '', actions: [{ id: 'ok', label: 'OK' }] });
    expect(store.current?.title).toBe('A');
    store.answer('ok');
    expect(await a).toBe('ok');
    expect(store.current).toBeNull();
    const b = store.ask({ title: 'B', body: '', actions: [] });
    store.answer(null);
    expect(await b).toBeNull();
  });

  it('queues concurrent requests', async () => {
    const store = new ConfirmStore();
    const a = store.ask({ title: 'A', body: '', actions: [] });
    const b = store.ask({ title: 'B', body: '', actions: [] });
    expect(store.current?.title).toBe('A');
    store.answer('x');
    expect(store.current?.title).toBe('B');
    store.answer('y');
    expect([await a, await b]).toEqual(['x', 'y']);
  });
});

describe('PromptStore', () => {
  it('returns the entered text and cancels a previous prompt', async () => {
    const store = new PromptStore();
    const first = store.ask({ title: 'Rename', label: 'Name', value: 'a' });
    const second = store.ask({ title: 'Rename', label: 'Name', value: 'b' });
    expect(await first).toBeNull();
    expect(store.current?.value).toBe('b');
    store.answer('renamed');
    expect(await second).toBe('renamed');
    expect(store.current).toBeNull();
  });
});
