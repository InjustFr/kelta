import { afterEach, describe, expect, it } from 'vitest';

import type { Layout } from '$lib/gen';
import { GLOBAL } from '$lib/stores';

import { seedGlobalLayout } from './nav';

const empty = (rev: number, active: string | null = null): Layout => ({
  project_id: GLOBAL,
  tabs: [],
  active_tab: active,
  rev,
});

describe('seedGlobalLayout (#270)', () => {
  afterEach(() => localStorage.clear());

  it('seeds a never-saved layout from the pre-#270 storage: views state and place', () => {
    localStorage.setItem('kelta.view.tickets', JSON.stringify({ who: 'anyone', mode: 'board' }));
    localStorage.setItem('kelta.view.reviews', JSON.stringify({ tab: 'team' }));
    localStorage.setItem('kelta.place', 'reviews');
    const l = seedGlobalLayout(empty(0));
    expect(
      l.tabs.map((t) => [
        t.id,
        t.root.type === 'pane' && t.root.id,
        t.root.type === 'pane' && t.root.content,
      ]),
    ).toEqual([
      [
        'tickets',
        'tickets',
        { kind: 'tickets', scope: { kind: 'all' }, view_id: null, who: 'anyone', mode: 'list' },
      ],
      ['now', 'now', { kind: 'inbox' }],
      ['reviews', 'reviews', { kind: 'reviews', scope: { kind: 'all' }, tab: 'team' }],
    ]);
    expect(l.active_tab).toBe('reviews');
    expect(seedGlobalLayout(l)).toBe(l);
  });

  it('a saved layout keeps its place; an unknown stored place is none', () => {
    localStorage.setItem('kelta.place', 'reviews');
    expect(seedGlobalLayout(empty(3)).active_tab).toBeNull();
    localStorage.setItem('kelta.place', 'bogus');
    expect(seedGlobalLayout(empty(0)).active_tab).toBeNull();
  });
});
