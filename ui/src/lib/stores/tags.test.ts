import { beforeEach, describe, expect, it } from 'vitest';

import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';

import { TagsStore } from './tags.svelte';

let mock: MockControls;
let tags: TagsStore;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  tags = new TagsStore();
  await tags.load();
  await tags.put(null, { name: 'retest', color: 'theme:red' });
  await tags.put(null, { name: 'ping', color: null });
});

describe('TagsStore', () => {
  it('tags tickets and PRs apart, counts them and keeps them across a reload', async () => {
    await tags.toggle('tickets', 'jira:A-1', 'ping');
    await tags.toggle('tickets', 'jira:A-1', 'retest');
    await tags.toggle('prs', 'gh:acme/shop#1', 'ping');
    expect(tags.of('tickets', 'jira:A-1').map((t) => t.name)).toEqual(['retest', 'ping']);
    expect(tags.counts('tickets', ['jira:A-1', 'jira:A-2']).map((c) => c.n)).toEqual([1, 1]);
    expect(tags.uses('ping')).toBe(2);

    const again = new TagsStore();
    await again.load();
    expect(again.has('prs', 'gh:acme/shop#1', 'ping')).toBe(true);
    // Only tags_get / tags_set: nothing goes to a tracker or code host.
    expect(new Set(mock.calls.map((c) => c.cmd))).toEqual(new Set(['tags_get', 'tags_set']));
  });

  it('a rename keeps the tag on its items; a delete takes it off and the filter lets go', async () => {
    await tags.toggle('tickets', 'jira:A-1', 'ping');
    await tags.put(1, { name: 'ping PO', color: 'theme:blue' });
    expect(tags.has('tickets', 'jira:A-1', 'ping PO')).toBe(true);
    expect(tags.live('ping')).toBeNull();

    await tags.remove('ping PO');
    expect(tags.value.tags.map((t) => t.name)).toEqual(['retest']);
    expect(tags.of('tickets', 'jira:A-1')).toEqual([]);
  });

  it('rolls back a refused save', async () => {
    let failed: unknown = null;
    tags.onSaveError = (err) => (failed = err);
    mock.failNext('tags_set', { code: 'invalid_argument', message: 'Two tags are named retest' });
    await tags.put(null, { name: 'retest', color: null });
    expect(failed).not.toBeNull();
    expect(tags.value.tags).toHaveLength(2);
  });
});
