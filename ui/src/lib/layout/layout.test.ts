import { describe, expect, it } from 'vitest';

import type { Layout, LayoutNode, PaneContent } from '$lib/gen';

import {
  MIN_RATIO,
  activeTab,
  addTab,
  allPanes,
  closePane,
  closePaneInLayout,
  closeTab,
  contentEquals,
  findPane,
  findSession,
  focusPane,
  makeTab,
  moveTab,
  neighbor,
  normalizeRatios,
  normalizeTree,
  openPane,
  pane,
  paneRects,
  replacePaneContent,
  resizeGutter,
  splitPane,
  toggleZoom,
  validateLayout,
  type PaneNode,
} from './index';

const term = (id: string): PaneContent => ({ kind: 'terminal', session_id: id });
const p = (id: string, session = id): PaneNode => pane(term(`s-${session}`), id);
const sum = (xs: number[]): number => xs.reduce((a, b) => a + b, 0);

function layoutOf(root: LayoutNode, extra: Partial<Layout> = {}): Layout {
  const tab = makeTab('T', root, { id: 't1' });
  return { project_id: 'shop', tabs: [tab], active_tab: 't1', rev: 1, ...extra };
}

describe('normalizeRatios', () => {
  it('scales to sum 1', () => {
    const r = normalizeRatios([2, 2]);
    expect(r).toEqual([0.5, 0.5]);
  });

  it('enforces the minimum share', () => {
    const r = normalizeRatios([0.99, 0.01]);
    expect(sum(r)).toBeCloseTo(1, 9);
    expect(Math.min(...r)).toBeGreaterThanOrEqual(MIN_RATIO - 1e-9);
  });

  it('falls back to equal shares on garbage', () => {
    expect(normalizeRatios([Number.NaN, 0, -1])).toEqual([1 / 3, 1 / 3, 1 / 3]);
    expect(normalizeRatios([])).toEqual([]);
    expect(normalizeRatios([42])).toEqual([1]);
  });

  it('caps the floor when there are many children', () => {
    const r = normalizeRatios(Array.from({ length: 30 }, (_, i) => (i === 0 ? 100 : 0.0001)));
    expect(sum(r)).toBeCloseTo(1, 9);
    expect(Math.min(...r)).toBeGreaterThan(0);
  });
});

describe('splitPane', () => {
  it('turns a pane into a split', () => {
    const root = splitPane(p('a'), 'a', 'row', p('b'));
    expect(root).toMatchObject({ type: 'split', dir: 'row', ratios: [0.5, 0.5] });
    expect(allPanes(root).map((x) => x.id)).toEqual(['a', 'b']);
  });

  it('inserts before when asked', () => {
    const root = splitPane(p('a'), 'a', 'column', p('b'), { before: true });
    expect(allPanes(root).map((x) => x.id)).toEqual(['b', 'a']);
  });

  it('adds a sibling to a same-direction parent and splits the share', () => {
    const one = splitPane(p('a'), 'a', 'row', p('b'));
    const two = splitPane(one, 'b', 'row', p('c'));
    expect(two.type).toBe('split');
    if (two.type !== 'split') return;
    expect(two.children).toHaveLength(3);
    expect(two.ratios[0]).toBeCloseTo(0.5);
    expect(two.ratios[1]).toBeCloseTo(0.25);
    expect(two.ratios[2]).toBeCloseTo(0.25);
  });

  it('nests when the direction differs', () => {
    const one = splitPane(p('a'), 'a', 'row', p('b'));
    const two = splitPane(one, 'b', 'column', p('c'));
    expect(findPane(two, 'c')?.path).toEqual([1, 1]);
  });

  it('is a no-op for an unknown pane', () => {
    const root = p('a');
    expect(splitPane(root, 'zzz', 'row', p('b'))).toBe(root);
  });

  it('does not mutate its input', () => {
    const root = splitPane(p('a'), 'a', 'row', p('b'));
    const snapshot = JSON.stringify(root);
    splitPane(root, 'a', 'row', p('c'));
    expect(JSON.stringify(root)).toBe(snapshot);
  });
});

describe('closePane', () => {
  it('returns null when the last pane closes', () => {
    expect(closePane(p('a'), 'a')).toBeNull();
  });

  it('collapses single-child splits', () => {
    const root = splitPane(p('a'), 'a', 'row', p('b'));
    expect(closePane(root, 'b')).toEqual(p('a'));
  });

  it('renormalizes remaining ratios', () => {
    let root = splitPane(p('a'), 'a', 'row', p('b'));
    root = splitPane(root, 'b', 'row', p('c'));
    const closed = closePane(root, 'a');
    expect(closed?.type).toBe('split');
    if (closed?.type !== 'split') return;
    expect(sum(closed.ratios)).toBeCloseTo(1);
    expect(closed.ratios[0]).toBeCloseTo(0.5);
  });

  it('flattens a nested split that becomes same-direction', () => {
    // row[a, column[b, row[c, d]]] → close b → row[a, c, d]
    let root = splitPane(p('a'), 'a', 'row', p('b'));
    root = splitPane(root, 'b', 'column', p('c'));
    root = splitPane(root, 'c', 'row', p('d'));
    const closed = closePane(root, 'b');
    expect(closed?.type).toBe('split');
    if (closed?.type !== 'split') return;
    expect(closed.dir).toBe('row');
    expect(closed.children.map((c) => (c.type === 'pane' ? c.id : 'split'))).toEqual(['a', 'c', 'd']);
    expect(sum(closed.ratios)).toBeCloseTo(1);
  });
});

describe('normalizeTree', () => {
  it('fixes bad ratio lists and singletons', () => {
    const bad: LayoutNode = {
      type: 'split',
      dir: 'row',
      ratios: [1],
      children: [{ type: 'split', dir: 'column', ratios: [0.3, 0.3], children: [p('a'), p('b')] }, p('c')],
    };
    const n = normalizeTree(bad);
    expect(validateLayout(layoutOf(n))).toEqual([]);
  });
});

describe('ratios', () => {
  const root = splitPane(p('a'), 'a', 'row', p('b'));

  it('resizeGutter clamps both sides to the minimum', () => {
    const r = resizeGutter(root, [], 0, 10);
    expect(r.type === 'split' && r.ratios[1]).toBeCloseTo(MIN_RATIO);
    const l = resizeGutter(root, [], 0, -10);
    expect(l.type === 'split' && l.ratios[0]).toBeCloseTo(MIN_RATIO);
  });
});

describe('geometry', () => {
  // row[a, column[b, c]]
  const root = splitPane(splitPane(p('a'), 'a', 'row', p('b')), 'b', 'column', p('c'));

  it('computes pane rects', () => {
    const rects = paneRects(root);
    expect(rects.get('a')).toEqual({ x: 0, y: 0, w: 0.5, h: 1 });
    expect(rects.get('c')).toEqual({ x: 0.5, y: 0.5, w: 0.5, h: 0.5 });
  });

  it('finds neighbours', () => {
    expect(neighbor(root, 'a', 'right')).toBe('b');
    expect(neighbor(root, 'c', 'left')).toBe('a');
    expect(neighbor(root, 'b', 'down')).toBe('c');
    expect(neighbor(root, 'c', 'up')).toBe('b');
    expect(neighbor(root, 'a', 'left')).toBeNull();
    expect(neighbor(root, 'b', 'up')).toBeNull();
  });
});

describe('tabs', () => {
  it('adds, activates, moves and closes tabs', () => {
    let l = layoutOf(p('a'));
    l = addTab(l, makeTab('Two', p('b'), { id: 't2' }));
    expect(l.active_tab).toBe('t2');
    l = moveTab(l, 't2', 0);
    expect(l.tabs.map((t) => t.id)).toEqual(['t2', 't1']);
    l = closeTab(l, 't2');
    expect(l.active_tab).toBe('t1');
    l = closeTab(l, 't1');
    expect(l.active_tab).toBeNull();
    expect(activeTab(l)).toBeNull();
  });

  it('closing the last pane removes the tab', () => {
    let l = layoutOf(p('a'));
    l = addTab(l, makeTab('Two', p('b'), { id: 't2' }));
    l = closePaneInLayout(l, 't2', 'b');
    expect(l.tabs.map((t) => t.id)).toEqual(['t1']);
  });

  it('keeps focus valid after closing the focused pane', () => {
    let l = layoutOf(splitPane(p('a'), 'a', 'row', p('b')));
    l = focusPane(l, 't1', 'b');
    l = closePaneInLayout(l, 't1', 'b');
    expect(l.tabs[0]?.focused_pane).toBe('a');
  });

  it('toggles zoom', () => {
    let l = layoutOf(splitPane(p('a'), 'a', 'row', p('b')));
    l = toggleZoom(l, 't1', 'b');
    expect(l.tabs[0]?.zoomed_pane).toBe('b');
    l = toggleZoom(l, 't1', 'b');
    expect(l.tabs[0]?.zoomed_pane).toBeNull();
  });
});

describe('sessions in layouts', () => {
  it('finds a session', () => {
    const l = layoutOf(splitPane(p('a', '1'), 'a', 'row', pane({ kind: 'empty' }, 'b')));
    expect(findSession(l, 's-1')).toEqual({ tabId: 't1', paneId: 'a' });
  });

  it('replacePaneContent swaps content only', () => {
    const root = replacePaneContent(p('a'), 'a', { kind: 'inbox' });
    expect(root).toEqual({ type: 'pane', id: 'a', content: { kind: 'inbox' } });
  });

  it('contentEquals ignores key order', () => {
    expect(
      contentEquals(
        { kind: 'tickets', scope: { kind: 'project', id: 'x' }, view_id: null, mode: 'list' },
        { mode: 'list', view_id: null, scope: { id: 'x', kind: 'project' }, kind: 'tickets' },
      ),
    ).toBe(true);
  });
});

describe('openPane', () => {
  const base = (): Layout => layoutOf(p('a', '1'));
  const req = (content: PaneContent, placement: Parameters<typeof openPane>[1]['placement']) => ({
    content,
    placement,
    focus: true,
    tab_title: null,
    work_item_id: null,
  });

  it('new_tab creates and activates a tab', () => {
    const { layout, location } = openPane(base(), req({ kind: 'inbox' }, 'new_tab'));
    expect(layout.tabs).toHaveLength(2);
    expect(layout.active_tab).toBe(location.tabId);
    expect(layout.tabs[1]?.title).toBe('Now');
  });

  it('split_right / split_down split the focused pane', () => {
    const right = openPane(base(), req({ kind: 'inbox' }, 'split_right'));
    expect(right.layout.tabs[0]?.root).toMatchObject({ type: 'split', dir: 'row' });
    expect(right.layout.tabs[0]?.focused_pane).toBe(right.location.paneId);
    const down = openPane(base(), req({ kind: 'inbox' }, 'split_down'));
    expect(down.layout.tabs[0]?.root).toMatchObject({ type: 'split', dir: 'column' });
  });

  it('replace_focused replaces the focused content', () => {
    const { layout } = openPane(base(), req({ kind: 'diagnostics' }, 'replace_focused'));
    expect(layout.tabs[0]?.root).toEqual({ type: 'pane', id: 'a', content: { kind: 'diagnostics' } });
  });

  it('focused re-uses an existing pane with the same content', () => {
    let l = openPane(base(), req({ kind: 'inbox' }, 'new_tab')).layout;
    l = { ...l, active_tab: 't1' };
    const again = openPane(l, req({ kind: 'inbox' }, 'focused'));
    expect(again.layout.tabs).toHaveLength(2);
    expect(again.layout.active_tab).toBe(again.location.tabId);
    expect(again.location.tabId).not.toBe('t1');
  });

  it('focused opens a new tab when nothing matches', () => {
    const { layout } = openPane(base(), req({ kind: 'reviews', scope: { kind: 'all' } }, 'focused'));
    expect(layout.tabs).toHaveLength(2);
  });

  it('moving a terminal elsewhere vacates its old pane', () => {
    const l = openPane(
      layoutOf(splitPane(p('a', '1'), 'a', 'row', p('b', '2'))),
      req(term('s-1'), 'new_tab'),
    ).layout;
    expect(validateLayout(l)).toEqual([]);
    expect(allPanes(l.tabs[0]!.root).map((x) => x.id)).toEqual(['b']);
    expect(findSession(l, 's-1')?.tabId).toBe(l.tabs[1]?.id);
  });

  it('works on an empty layout', () => {
    const { layout } = openPane(
      { project_id: 'x', tabs: [], active_tab: null, rev: 0 },
      req({ kind: 'welcome' }, 'split_right'),
    );
    expect(layout.tabs).toHaveLength(1);
    expect(validateLayout(layout)).toEqual([]);
  });
});

describe('validateLayout', () => {
  it('reports broken invariants', () => {
    const broken: Layout = {
      project_id: 'x',
      rev: 0,
      active_tab: 'nope',
      tabs: [
        {
          id: 't',
          title: 't',
          work_item_id: null,
          focused_pane: 'zz',
          zoomed_pane: null,
          root: { type: 'split', dir: 'row', ratios: [0.9, 0.9], children: [p('a', '1'), p('a', '1')] },
        },
      ],
    };
    const problems = validateLayout(broken);
    expect(problems.some((x) => x.includes('ratios sum'))).toBe(true);
    expect(problems.some((x) => x.includes('duplicate pane id'))).toBe(true);
    expect(problems.some((x) => x.includes('shown twice'))).toBe(true);
    expect(problems).toContain('active tab missing');
  });
});
