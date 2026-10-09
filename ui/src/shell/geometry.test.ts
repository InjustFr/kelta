import { describe, expect, it } from 'vitest';

import type { LayoutNode } from '$lib/gen';
import { MIN_RATIO, pane, resizeGutter } from '$lib/layout';

import { applyZoom, dragDelta, layoutGeometry } from './geometry';

const terminal = (id: string): ReturnType<typeof pane> =>
  pane({ kind: 'terminal', session_id: `s-${id}` }, id);

describe('layoutGeometry', () => {
  it('places a single pane on the whole area', () => {
    const g = layoutGeometry(terminal('a'));
    expect(g.panes).toHaveLength(1);
    expect(g.panes[0]!.rect).toEqual({ x: 0, y: 0, w: 1, h: 1 });
    expect(g.gutters).toEqual([]);
  });

  it('splits rows and columns by ratio and emits one gutter per boundary', () => {
    const root: LayoutNode = {
      type: 'split',
      dir: 'row',
      ratios: [0.25, 0.75],
      children: [
        terminal('a'),
        { type: 'split', dir: 'column', ratios: [0.6, 0.4], children: [terminal('b'), terminal('c')] },
      ],
    };
    const g = layoutGeometry(root);
    const rect = (id: string) => g.panes.find((p) => p.pane.id === id)!.rect;
    expect(rect('a')).toEqual({ x: 0, y: 0, w: 0.25, h: 1 });
    expect(rect('b').x).toBeCloseTo(0.25);
    expect(rect('b').w).toBeCloseTo(0.75);
    expect(rect('b').h).toBeCloseTo(0.6);
    expect(rect('c').y).toBeCloseTo(0.6);
    expect(g.gutters).toHaveLength(2);
    const [vertical, horizontal] = g.gutters;
    expect(vertical).toMatchObject({ path: [], index: 0, dir: 'row' });
    expect(vertical!.line.x).toBeCloseTo(0.25);
    expect(vertical!.line.w).toBe(0);
    expect(horizontal).toMatchObject({ path: [1], index: 0, dir: 'column' });
    expect(horizontal!.line.y).toBeCloseTo(0.6);
    expect(horizontal!.line.x).toBeCloseTo(0.25);
    expect(horizontal!.line.w).toBeCloseTo(0.75);
  });

  it('survives ratios that do not match the children (equal shares)', () => {
    const root: LayoutNode = {
      type: 'split',
      dir: 'row',
      ratios: [1],
      children: [terminal('a'), terminal('b')],
    };
    const g = layoutGeometry(root);
    expect(g.panes.map((p) => p.rect.w)).toEqual([0.5, 0.5]);
  });

  it('every pane rectangle stays inside the unit square', () => {
    const root: LayoutNode = {
      type: 'split',
      dir: 'column',
      ratios: [0.3, 0.3, 0.4],
      children: [
        terminal('a'),
        { type: 'split', dir: 'row', ratios: [0.5, 0.5], children: [terminal('b'), terminal('c')] },
        terminal('d'),
      ],
    };
    for (const { rect } of layoutGeometry(root).panes) {
      expect(rect.x).toBeGreaterThanOrEqual(0);
      expect(rect.y).toBeGreaterThanOrEqual(0);
      expect(rect.x + rect.w).toBeLessThanOrEqual(1 + 1e-9);
      expect(rect.y + rect.h).toBeLessThanOrEqual(1 + 1e-9);
    }
  });
});

describe('applyZoom', () => {
  const root: LayoutNode = {
    type: 'split',
    dir: 'row',
    ratios: [0.5, 0.5],
    children: [terminal('a'), terminal('b')],
  };

  it('gives the zoomed pane the whole area and removes gutters', () => {
    const z = applyZoom(layoutGeometry(root), 'b');
    expect(z.panes.find((p) => p.pane.id === 'b')!.rect).toEqual({ x: 0, y: 0, w: 1, h: 1 });
    expect(z.gutters).toEqual([]);
  });

  it('ignores a zoom target that no longer exists', () => {
    const g = layoutGeometry(root);
    expect(applyZoom(g, 'gone')).toBe(g);
    expect(applyZoom(g, null)).toBe(g);
  });
});

describe('dragDelta', () => {
  it('converts pixels to a fraction of the split span', () => {
    expect(dragDelta(100, 1000, { x: 0, y: 0, w: 1, h: 1 }, 'row')).toBeCloseTo(0.1);
    // Nested split covering half the area: the same pixels move the gutter twice as far.
    expect(dragDelta(100, 1000, { x: 0, y: 0, w: 0.5, h: 1 }, 'row')).toBeCloseTo(0.2);
    expect(dragDelta(30, 600, { x: 0, y: 0, w: 1, h: 0.5 }, 'column')).toBeCloseTo(0.1);
    expect(dragDelta(10, 0, { x: 0, y: 0, w: 1, h: 1 }, 'row')).toBe(0);
  });

  it('resizeGutter never takes a neighbour below the 5 % floor', () => {
    const root: LayoutNode = {
      type: 'split',
      dir: 'row',
      ratios: [0.5, 0.5],
      children: [terminal('a'), terminal('b')],
    };
    const wide = resizeGutter(root, [], 0, 10);
    const narrow = resizeGutter(root, [], 0, -10);
    expect(wide.type === 'split' && wide.ratios[1]).toBeCloseTo(MIN_RATIO);
    expect(narrow.type === 'split' && narrow.ratios[0]).toBeCloseTo(MIN_RATIO);
  });
});
