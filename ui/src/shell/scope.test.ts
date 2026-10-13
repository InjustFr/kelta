import { beforeEach, describe, expect, it, vi } from 'vitest';

import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { layout, projects } from '$lib/stores';
import { makeTab, pane } from '$lib/layout';

import { ListScope, listScope } from './scope.svelte';

let mock: MockControls;

beforeEach(async () => {
  localStorage.clear();
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  await projects.load();
});

describe('ListScope (#409)', () => {
  it('is all projects, or one, and survives a restart', () => {
    const scope = new ListScope();
    expect(scope.scope).toEqual({ kind: 'all' });
    scope.set('shop');
    expect(scope.scope).toEqual({ kind: 'project', id: 'shop' });
    expect(new ListScope().project).toBe('shop');
    scope.set(null);
    expect(new ListScope().scope).toEqual({ kind: 'all' });
  });

  it('Home, an unknown or a closed project is all projects', () => {
    const scope = new ListScope();
    scope.set(projects.home!.id);
    expect(scope.project).toBeNull();
    scope.set('gone');
    expect(scope.project).toBeNull();
    scope.set('billing');
    projects.list = projects.list.map((p) => (p.id === 'billing' ? { ...p, open: false } : p));
    expect(scope.project).toBeNull();
  });

  it('works without storage', () => {
    const spy = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('denied');
    });
    const scope = new ListScope();
    scope.set('shop');
    expect(scope.project).toBe('shop');
    spy.mockRestore();
  });

  it('a migrated Reviews tab scopes the lists to its project, once', async () => {
    listScope.set(null);
    const billing = mock.state.layouts.billing!;
    mock.state.layouts.billing = {
      ...billing,
      tabs: [
        ...billing.tabs,
        makeTab('Reviews', pane({ kind: 'reviews', scope: { kind: 'project', id: 'billing' } })),
      ],
    };
    await layout.load('billing');
    expect(listScope.project).toBe('billing');
    expect(layout.get('billing')!.tabs).toHaveLength(billing.tabs.length);
  });
});
