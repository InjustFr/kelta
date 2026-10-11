// Pure geometry of a layout tree (ARCHITECTURE §5.1): pane rectangles and gutters in the unit
// square. The renderer positions every pane absolutely from these numbers (flat DOM keyed by pane
// id), so a pane's DOM — and the terminal inside it — survives splits, closes and zoom.

import type { LayoutNode, PaneId, SplitDir } from '$lib/gen';
import {
  equalRatios,
  normalizeRatios,
  type DropZone,
  type NodePath,
  type PaneNode,
  type Rect,
} from '$lib/layout';

export interface PlacedPane {
  pane: PaneNode;
  rect: Rect;
}

export interface Gutter {
  /** Path of the split node this gutter belongs to. */
  path: NodePath;
  /** The gutter sits between children `index` and `index + 1`. */
  index: number;
  dir: SplitDir;
  /** Gutter line in the unit square (zero width for row splits, zero height for column splits). */
  line: Rect;
  /** Rectangle of the whole split node (to convert pixel drags into ratio deltas). */
  split: Rect;
}

export interface Geometry {
  panes: PlacedPane[];
  gutters: Gutter[];
}

export function layoutGeometry(root: LayoutNode): Geometry {
  const panes: PlacedPane[] = [];
  const gutters: Gutter[] = [];
  const visit = (node: LayoutNode, rect: Rect, path: NodePath): void => {
    if (node.type === 'pane') {
      panes.push({ pane: node, rect });
      return;
    }
    const shares =
      node.ratios.length === node.children.length
        ? normalizeRatios(node.ratios, 0)
        : equalRatios(node.children.length);
    let offset = 0;
    node.children.forEach((child, i) => {
      const share = shares[i] ?? 0;
      const childRect: Rect =
        node.dir === 'row'
          ? { x: rect.x + rect.w * offset, y: rect.y, w: rect.w * share, h: rect.h }
          : { x: rect.x, y: rect.y + rect.h * offset, w: rect.w, h: rect.h * share };
      offset += share;
      visit(child, childRect, [...path, i]);
      if (i < node.children.length - 1) {
        const line: Rect =
          node.dir === 'row'
            ? { x: rect.x + rect.w * offset, y: rect.y, w: 0, h: rect.h }
            : { x: rect.x, y: rect.y + rect.h * offset, w: rect.w, h: 0 };
        gutters.push({ path, index: i, dir: node.dir, line, split: rect });
      }
    });
  };
  visit(root, { x: 0, y: 0, w: 1, h: 1 }, []);
  return { panes, gutters };
}

/** The zoomed pane fills the area; every other pane is hidden. */
export function applyZoom(geometry: Geometry, zoomed: PaneId | null): Geometry {
  if (!zoomed || !geometry.panes.some((p) => p.pane.id === zoomed)) return geometry;
  return {
    panes: geometry.panes.map((p) =>
      p.pane.id === zoomed ? { pane: p.pane, rect: { x: 0, y: 0, w: 1, h: 1 } } : p,
    ),
    gutters: [],
  };
}

/**
 * Ratio delta for a pointer drag of `deltaPx` along the split axis. `areaPx` is the pixel size of
 * the whole workspace along that axis, `split` the split's rectangle in the unit square.
 */
export function dragDelta(deltaPx: number, areaPx: number, split: Rect, dir: SplitDir): number {
  const span = (dir === 'row' ? split.w : split.h) * areaPx;
  return span > 0 ? deltaPx / span : 0;
}

/**
 * Pane under the point (`x`, `y` in the unit square) and the zone a dragged pane would take: the
 * nearest edge, or the centre in the middle half. `area` is the part of the pane the drop indicator covers.
 */
export function dropTarget(
  panes: readonly PlacedPane[],
  x: number,
  y: number,
): { pane: PaneNode; zone: DropZone; area: Rect } | null {
  const hit = panes.find(({ rect: r }) => x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h);
  if (!hit) return null;
  const r = hit.rect;
  const u = r.w > 0 ? (x - r.x) / r.w : 0.5;
  const v = r.h > 0 ? (y - r.y) / r.h : 0.5;
  const edges: [DropZone, number][] = [
    ['left', u],
    ['right', 1 - u],
    ['top', v],
    ['bottom', 1 - v],
  ];
  const [edge, dist] = edges.reduce((a, b) => (b[1] < a[1] ? b : a));
  const zone: DropZone = dist > 0.25 ? 'center' : edge;
  const areas: Record<DropZone, Rect> = {
    left: { ...r, w: r.w / 2 },
    right: { ...r, x: r.x + r.w / 2, w: r.w / 2 },
    top: { ...r, h: r.h / 2 },
    bottom: { ...r, y: r.y + r.h / 2, h: r.h / 2 },
    center: r,
  };
  const area = areas[zone];
  return { pane: hit.pane, zone, area };
}
