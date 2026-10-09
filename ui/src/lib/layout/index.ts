// Pure layout tree operations (ARCHITECTURE §5.1). No Svelte, no IPC: every function returns a
// new value and never mutates its input. Used by the layout store, the shell and the IPC mock.

import type {
  Layout,
  LayoutNode,
  OpenPaneRequest,
  PaneContent,
  PaneId,
  SessionId,
  SplitDir,
  Tab,
  TabId,
} from '$lib/gen';

export type PaneNode = Extract<LayoutNode, { type: 'pane' }>;
export type SplitNode = Extract<LayoutNode, { type: 'split' }>;
/** Child indices from the root to a node. `[]` is the root. */
export type NodePath = readonly number[];
export type Direction = 'left' | 'right' | 'up' | 'down';
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}
export interface PaneLocation {
  tabId: TabId;
  paneId: PaneId;
}

/** Minimum share of a split child (ARCH §5.1: each ratio ≥ 0.05). */
export const MIN_RATIO = 0.05;
const EPS = 1e-6;

let idCounter = 0;
/** Unique id for tabs and panes (`<prefix>-<random>`). */
export function newId(prefix: 'tab' | 'pane'): string {
  idCounter += 1;
  const rand =
    typeof crypto !== 'undefined' && 'randomUUID' in crypto
      ? crypto.randomUUID().slice(0, 8)
      : Math.random().toString(16).slice(2, 10);
  return `${prefix}-${rand}${idCounter.toString(36)}`;
}

export function pane(content: PaneContent, id: PaneId = newId('pane')): PaneNode {
  return { type: 'pane', id, content };
}

// ---- ratios ---------------------------------------------------------------------------------

export function equalRatios(n: number): number[] {
  return n <= 0 ? [] : Array.from({ length: n }, () => 1 / n);
}

/**
 * Returns ratios that are finite, ≥ `min` each and sum to 1, keeping the given proportions as far as
 * possible. Invalid input (empty sum, NaN) falls back to equal shares.
 */
export function normalizeRatios(ratios: readonly number[], min: number = MIN_RATIO): number[] {
  const n = ratios.length;
  if (n === 0) return [];
  if (n === 1) return [1];
  const floor = Math.min(min, 1 / n);
  const clean = ratios.map((r) => (Number.isFinite(r) && r > 0 ? r : 0));
  const sum = clean.reduce((a, b) => a + b, 0);
  if (sum <= EPS) return equalRatios(n);
  let out = clean.map((r) => r / sum);
  // Raise small entries to the floor and take the difference proportionally from the others.
  for (let guard = 0; guard < n; guard += 1) {
    const low = out.map((r) => r < floor - EPS);
    if (!low.some(Boolean)) break;
    const fixed = low.filter(Boolean).length * floor;
    const restSum = out.reduce((a, r, i) => (low[i] ? a : a + r), 0);
    const scale = restSum > EPS ? (1 - fixed) / restSum : 0;
    out = out.map((r, i) => (low[i] ? floor : r * scale));
  }
  const total = out.reduce((a, b) => a + b, 0);
  return out.map((r) => r / total);
}

// ---- traversal ------------------------------------------------------------------------------

export function walk(
  node: LayoutNode,
  visit: (node: LayoutNode, path: NodePath) => void,
  path: NodePath = [],
): void {
  visit(node, path);
  if (node.type === 'split') node.children.forEach((child, i) => walk(child, visit, [...path, i]));
}

export function allPanes(node: LayoutNode): PaneNode[] {
  const out: PaneNode[] = [];
  walk(node, (n) => {
    if (n.type === 'pane') out.push(n);
  });
  return out;
}

export function nodeAt(root: LayoutNode, path: NodePath): LayoutNode | null {
  let node: LayoutNode = root;
  for (const i of path) {
    if (node.type !== 'split') return null;
    const child: LayoutNode | undefined = node.children[i];
    if (!child) return null;
    node = child;
  }
  return node;
}

export function findPaneBy(
  root: LayoutNode,
  predicate: (pane: PaneNode) => boolean,
): { pane: PaneNode; path: NodePath } | null {
  let found: { pane: PaneNode; path: NodePath } | null = null;
  walk(root, (n, path) => {
    if (!found && n.type === 'pane' && predicate(n)) found = { pane: n, path };
  });
  return found;
}

export function findPane(root: LayoutNode, paneId: PaneId): { pane: PaneNode; path: NodePath } | null {
  return findPaneBy(root, (p) => p.id === paneId);
}

/** Deep structural equality of pane contents (key order independent). */
export function contentEquals(a: PaneContent, b: PaneContent): boolean {
  return deepEqual(a, b);
}

function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const ka = Object.keys(a as object);
  const kb = Object.keys(b as object);
  if (ka.length !== kb.length) return false;
  return ka.every((k) => deepEqual((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]));
}

/** Session id shown by a pane, if it is a terminal pane. */
export function paneSession(p: PaneNode): SessionId | null {
  return p.content.kind === 'terminal' ? p.content.session_id : null;
}

// ---- immutable updates ----------------------------------------------------------------------

export function replaceAt(root: LayoutNode, path: NodePath, replacement: LayoutNode): LayoutNode {
  if (path.length === 0) return replacement;
  if (root.type !== 'split') return root;
  const [head, ...rest] = path;
  if (head === undefined || head < 0 || head >= root.children.length) return root;
  const children = root.children.map((c, i) => (i === head ? replaceAt(c, rest, replacement) : c));
  return { ...root, children };
}

export function replacePaneContent(root: LayoutNode, paneId: PaneId, content: PaneContent): LayoutNode {
  const hit = findPane(root, paneId);
  if (!hit) return root;
  return replaceAt(root, hit.path, { ...hit.pane, content });
}

/**
 * Splits `paneId` in direction `dir` (`row` = side by side, `column` = stacked) and inserts
 * `newPane` after it (or before with `before: true`). When the parent split already has the same
 * direction the new pane becomes a sibling and takes `ratio` of the target's share.
 */
export function splitPane(
  root: LayoutNode,
  paneId: PaneId,
  dir: SplitDir,
  newPane: PaneNode,
  opts: { before?: boolean; ratio?: number } = {},
): LayoutNode {
  const hit = findPane(root, paneId);
  if (!hit) return root;
  const share = clamp(opts.ratio ?? 0.5, MIN_RATIO, 1 - MIN_RATIO);
  const parentPath = hit.path.slice(0, -1);
  const index = hit.path[hit.path.length - 1];
  const parent = hit.path.length > 0 ? nodeAt(root, parentPath) : null;
  if (parent && parent.type === 'split' && parent.dir === dir && index !== undefined) {
    const old = parent.ratios[index] ?? 1 / parent.children.length;
    const children = [...parent.children];
    const ratios = [...parent.ratios];
    const insertAt = opts.before ? index : index + 1;
    children.splice(insertAt, 0, newPane);
    ratios.splice(index, 1, old * (1 - share));
    ratios.splice(insertAt, 0, old * share);
    return replaceAt(root, parentPath, { ...parent, children, ratios: normalizeRatios(ratios) });
  }
  const pair = opts.before ? [newPane, hit.pane] : [hit.pane, newPane];
  const ratios = opts.before ? [share, 1 - share] : [1 - share, share];
  return replaceAt(root, hit.path, { type: 'split', dir, ratios, children: pair });
}

/** Removes a pane. Returns `null` when the tree becomes empty. The result is normalized. */
export function closePane(root: LayoutNode, paneId: PaneId): LayoutNode | null {
  const removed = removeNode(root, (n) => n.type === 'pane' && n.id === paneId);
  return removed ? normalizeTree(removed) : null;
}

function removeNode(node: LayoutNode, match: (n: LayoutNode) => boolean): LayoutNode | null {
  if (match(node)) return null;
  if (node.type === 'pane') return node;
  const children: LayoutNode[] = [];
  const ratios: number[] = [];
  node.children.forEach((child, i) => {
    const next = removeNode(child, match);
    if (next) {
      children.push(next);
      ratios.push(node.ratios[i] ?? 0);
    }
  });
  if (children.length === 0) return null;
  return { ...node, children, ratios };
}

/**
 * Canonical form: single-child splits collapse, nested splits with the same direction are flattened
 * (ratios multiplied through) and every ratio list is normalized.
 */
export function normalizeTree(node: LayoutNode): LayoutNode {
  if (node.type === 'pane') return node;
  const baseRatios =
    node.ratios.length === node.children.length ? node.ratios : equalRatios(node.children.length);
  const shares = normalizeRatios(baseRatios, 0);
  const children: LayoutNode[] = [];
  const ratios: number[] = [];
  node.children.forEach((child, i) => {
    const c = normalizeTree(child);
    const share = shares[i] ?? 0;
    if (c.type === 'split' && c.dir === node.dir) {
      c.children.forEach((gc, j) => {
        children.push(gc);
        ratios.push(share * (c.ratios[j] ?? 0));
      });
    } else {
      children.push(c);
      ratios.push(share);
    }
  });
  if (children.length === 1 && children[0]) return children[0];
  return { type: 'split', dir: node.dir, children, ratios: normalizeRatios(ratios) };
}

/** Sets the ratios of the split at `path` (normalized). */
export function setSplitRatios(root: LayoutNode, path: NodePath, ratios: readonly number[]): LayoutNode {
  const node = nodeAt(root, path);
  if (!node || node.type !== 'split' || ratios.length !== node.children.length) return root;
  return replaceAt(root, path, { ...node, ratios: normalizeRatios(ratios) });
}

/**
 * Moves the gutter between child `index` and `index + 1` of the split at `path` by `delta`
 * (fraction of the split size). Both neighbours stay ≥ MIN_RATIO.
 */
export function resizeGutter(root: LayoutNode, path: NodePath, index: number, delta: number): LayoutNode {
  const node = nodeAt(root, path);
  if (!node || node.type !== 'split') return root;
  const a = node.ratios[index];
  const b = node.ratios[index + 1];
  if (a === undefined || b === undefined) return root;
  const pairTotal = a + b;
  const nextA = clamp(a + delta, MIN_RATIO, pairTotal - MIN_RATIO);
  const ratios = [...node.ratios];
  ratios[index] = nextA;
  ratios[index + 1] = pairTotal - nextA;
  return replaceAt(root, path, { ...node, ratios });
}

// ---- geometry -------------------------------------------------------------------------------

/** Pane rectangles in the unit square (x right, y down). */
export function paneRects(root: LayoutNode, rect: Rect = { x: 0, y: 0, w: 1, h: 1 }): Map<PaneId, Rect> {
  const out = new Map<PaneId, Rect>();
  const visit = (node: LayoutNode, r: Rect): void => {
    if (node.type === 'pane') {
      out.set(node.id, r);
      return;
    }
    const ratios = normalizeRatios(node.ratios.length === node.children.length ? node.ratios : [], 0);
    const shares = ratios.length === node.children.length ? ratios : equalRatios(node.children.length);
    let offset = 0;
    node.children.forEach((child, i) => {
      const s = shares[i] ?? 0;
      const childRect =
        node.dir === 'row'
          ? { x: r.x + r.w * offset, y: r.y, w: r.w * s, h: r.h }
          : { x: r.x, y: r.y + r.h * offset, w: r.w, h: r.h * s };
      offset += s;
      visit(child, childRect);
    });
  };
  visit(root, rect);
  return out;
}

/** Nearest pane in a direction (overlapping on the other axis), or null. */
export function neighbor(root: LayoutNode, paneId: PaneId, direction: Direction): PaneId | null {
  const rects = paneRects(root);
  const from = rects.get(paneId);
  if (!from) return null;
  const cx = from.x + from.w / 2;
  const cy = from.y + from.h / 2;
  let best: { id: PaneId; dist: number; overlap: number } | null = null;
  for (const [id, r] of rects) {
    if (id === paneId) continue;
    let dist: number;
    let overlap: number;
    switch (direction) {
      case 'left':
        dist = from.x - (r.x + r.w);
        overlap = Math.min(from.y + from.h, r.y + r.h) - Math.max(from.y, r.y);
        break;
      case 'right':
        dist = r.x - (from.x + from.w);
        overlap = Math.min(from.y + from.h, r.y + r.h) - Math.max(from.y, r.y);
        break;
      case 'up':
        dist = from.y - (r.y + r.h);
        overlap = Math.min(from.x + from.w, r.x + r.w) - Math.max(from.x, r.x);
        break;
      case 'down':
        dist = r.y - (from.y + from.h);
        overlap = Math.min(from.x + from.w, r.x + r.w) - Math.max(from.x, r.x);
        break;
    }
    if (dist < -EPS || overlap <= EPS) continue;
    const centerDist =
      direction === 'left' || direction === 'right'
        ? Math.abs(r.y + r.h / 2 - cy)
        : Math.abs(r.x + r.w / 2 - cx);
    const score = dist + centerDist * 1e-3;
    if (!best || score < best.dist || (Math.abs(score - best.dist) < EPS && overlap > best.overlap)) {
      best = { id, dist: score, overlap };
    }
  }
  return best ? best.id : null;
}

// ---- tabs and layouts -----------------------------------------------------------------------

export function emptyLayout(projectId: string): Layout {
  return { project_id: projectId, tabs: [], active_tab: null, rev: 0 };
}

export function findTab(layout: Layout, tabId: TabId): Tab | null {
  return layout.tabs.find((t) => t.id === tabId) ?? null;
}

export function activeTab(layout: Layout): Tab | null {
  return (layout.active_tab ? findTab(layout, layout.active_tab) : null) ?? layout.tabs[0] ?? null;
}

export function updateTab(layout: Layout, tabId: TabId, fn: (tab: Tab) => Tab): Layout {
  return { ...layout, tabs: layout.tabs.map((t) => (t.id === tabId ? fn(t) : t)) };
}

export function makeTab(
  title: string,
  root: LayoutNode,
  opts: { id?: TabId; work_item_id?: string | null } = {},
): Tab {
  const first = allPanes(root)[0];
  return {
    id: opts.id ?? newId('tab'),
    title,
    work_item_id: opts.work_item_id ?? null,
    root,
    focused_pane: first ? first.id : null,
    zoomed_pane: null,
  };
}

export function addTab(layout: Layout, tab: Tab, opts: { activate?: boolean; index?: number } = {}): Layout {
  const tabs = [...layout.tabs];
  tabs.splice(opts.index ?? tabs.length, 0, tab);
  return { ...layout, tabs, active_tab: opts.activate === false ? layout.active_tab : tab.id };
}

export function closeTab(layout: Layout, tabId: TabId): Layout {
  const index = layout.tabs.findIndex((t) => t.id === tabId);
  if (index < 0) return layout;
  const tabs = layout.tabs.filter((t) => t.id !== tabId);
  let active = layout.active_tab;
  if (active === tabId) {
    const next = tabs[Math.min(index, tabs.length - 1)];
    active = next ? next.id : null;
  }
  return { ...layout, tabs, active_tab: active };
}

export function activateTab(layout: Layout, tabId: TabId): Layout {
  return findTab(layout, tabId) ? { ...layout, active_tab: tabId } : layout;
}

export function moveTab(layout: Layout, tabId: TabId, toIndex: number): Layout {
  const from = layout.tabs.findIndex((t) => t.id === tabId);
  if (from < 0) return layout;
  const tabs = [...layout.tabs];
  const [tab] = tabs.splice(from, 1);
  if (!tab) return layout;
  tabs.splice(clamp(toIndex, 0, tabs.length), 0, tab);
  return { ...layout, tabs };
}

export function focusPane(layout: Layout, tabId: TabId, paneId: PaneId): Layout {
  const tab = findTab(layout, tabId);
  if (!tab || !findPane(tab.root, paneId)) return layout;
  return { ...updateTab(layout, tabId, (t) => ({ ...t, focused_pane: paneId })), active_tab: tabId };
}

export function toggleZoom(layout: Layout, tabId: TabId, paneId?: PaneId): Layout {
  return updateTab(layout, tabId, (t) => {
    const target = paneId ?? t.focused_pane;
    if (!target) return t;
    return { ...t, zoomed_pane: t.zoomed_pane === target ? null : target };
  });
}

/** Closes a pane inside a tab; the tab is removed when its last pane closes. */
export function closePaneInLayout(layout: Layout, tabId: TabId, paneId: PaneId): Layout {
  const tab = findTab(layout, tabId);
  if (!tab) return layout;
  const root = closePane(tab.root, paneId);
  if (!root) return closeTab(layout, tabId);
  return updateTab(layout, tabId, (t) => fixTabRefs({ ...t, root }));
}

/** Keeps focused/zoomed pane ids valid after structural changes. */
function fixTabRefs(tab: Tab): Tab {
  const ids = new Set(allPanes(tab.root).map((p) => p.id));
  const first = allPanes(tab.root)[0];
  return {
    ...tab,
    focused_pane: tab.focused_pane && ids.has(tab.focused_pane) ? tab.focused_pane : first ? first.id : null,
    zoomed_pane: tab.zoomed_pane && ids.has(tab.zoomed_pane) ? tab.zoomed_pane : null,
  };
}

/** Where a session is shown in this layout (a session is in at most one pane). */
export function findSession(layout: Layout, sessionId: SessionId): PaneLocation | null {
  return findContent(layout, (c) => c.kind === 'terminal' && c.session_id === sessionId);
}

export function findContent(
  layout: Layout,
  predicate: (content: PaneContent) => boolean,
): PaneLocation | null {
  for (const tab of layout.tabs) {
    const hit = findPaneBy(tab.root, (p) => predicate(p.content));
    if (hit) return { tabId: tab.id, paneId: hit.pane.id };
  }
  return null;
}

/** Every session id referenced by the layout. */
export function layoutSessions(layout: Layout): SessionId[] {
  const out: SessionId[] = [];
  for (const tab of layout.tabs) {
    for (const p of allPanes(tab.root)) {
      const s = paneSession(p);
      if (s) out.push(s);
    }
  }
  return out;
}

/**
 * Shows `sessionId` in the target pane (replacing its content). If the session was shown in
 * another pane, that pane is closed (a session lives in at most one pane).
 */
export function moveSession(layout: Layout, sessionId: SessionId, target: PaneLocation): Layout {
  const from = findSession(layout, sessionId);
  if (from && from.tabId === target.tabId && from.paneId === target.paneId) return layout;
  let next = updateTab(layout, target.tabId, (t) => ({
    ...t,
    root: replacePaneContent(t.root, target.paneId, { kind: 'terminal', session_id: sessionId }),
  }));
  if (from) next = closePaneInLayout(next, from.tabId, from.paneId);
  return next;
}

export function defaultTitle(content: PaneContent): string {
  switch (content.kind) {
    case 'terminal':
      return 'Terminal';
    case 'web':
      return 'Web';
    case 'plugin_screen':
      return content.screen_id;
    case 'tickets':
      return content.mode === 'board' ? 'Board' : 'Tickets';
    case 'ticket_detail':
      return content.ticket.key;
    case 'reviews':
      return 'Reviews';
    case 'review_detail':
      return `${content.review.repo}#${content.review.number}`;
    case 'inbox':
      return 'Inbox';
    case 'work_item':
      return 'Work item';
    case 'settings':
      return 'Settings';
    case 'diagnostics':
      return 'Diagnostics';
    case 'welcome':
      return 'Welcome';
    case 'empty':
      return 'Empty';
  }
}

/**
 * Applies an `OpenPaneRequest` (ARCH §5.1 Placement). Terminal contents keep the "at most one pane"
 * rule: an existing pane showing the session is focused (`focused`) or vacated (other placements).
 */
export function openPane(
  layout: Layout,
  req: OpenPaneRequest,
  ids: { tabId?: TabId; paneId?: PaneId } = {},
): { layout: Layout; location: PaneLocation } {
  const existing = findContent(layout, (c) => contentEquals(c, req.content));
  if (existing && req.placement === 'focused') {
    const next = req.focus ? focusPane(layout, existing.tabId, existing.paneId) : layout;
    return { layout: next, location: existing };
  }
  const newPaneNode = pane(req.content, ids.paneId ?? newId('pane'));
  const current = activeTab(layout);
  const focused = current?.focused_pane ?? (current ? allPanes(current.root)[0]?.id : undefined) ?? null;
  let next: Layout;
  let location: PaneLocation;

  if (!current || !focused || req.placement === 'new_tab' || req.placement === 'focused') {
    const tab = makeTab(req.tab_title ?? defaultTitle(req.content), newPaneNode, {
      id: ids.tabId,
      work_item_id: req.work_item_id,
    });
    next = addTab(layout, tab, { activate: req.focus || layout.tabs.length === 0 });
    location = { tabId: tab.id, paneId: newPaneNode.id };
  } else if (req.placement === 'replace_focused') {
    next = updateTab(layout, current.id, (t) => ({
      ...t,
      root: replacePaneContent(t.root, focused, req.content),
    }));
    location = { tabId: current.id, paneId: focused };
  } else {
    const dir: SplitDir = req.placement === 'split_right' ? 'row' : 'column';
    next = updateTab(layout, current.id, (t) => ({
      ...t,
      root: splitPane(t.root, focused, dir, newPaneNode),
    }));
    location = { tabId: current.id, paneId: newPaneNode.id };
  }
  const samePane = existing && existing.tabId === location.tabId && existing.paneId === location.paneId;
  if (existing && !samePane && req.content.kind === 'terminal') {
    next = closePaneInLayout(next, existing.tabId, existing.paneId);
  }
  if (req.focus) next = focusPane(next, location.tabId, location.paneId);
  return { layout: next, location };
}

/** Checks the structural invariants (used by tests and dev assertions). Returns problems found. */
export function validateLayout(layout: Layout): string[] {
  const problems: string[] = [];
  const paneIds = new Set<string>();
  const sessions = new Set<string>();
  for (const tab of layout.tabs) {
    walk(tab.root, (n, path) => {
      if (n.type === 'split') {
        if (n.children.length < 2) problems.push(`${tab.id}:${path.join('.')} split with < 2 children`);
        if (n.ratios.length !== n.children.length)
          problems.push(`${tab.id}:${path.join('.')} ratios/children mismatch`);
        const sum = n.ratios.reduce((a, b) => a + b, 0);
        if (Math.abs(sum - 1) > 1e-3) problems.push(`${tab.id}:${path.join('.')} ratios sum ${sum}`);
        if (n.ratios.some((r) => r < MIN_RATIO - 1e-4))
          problems.push(`${tab.id}:${path.join('.')} ratio < ${MIN_RATIO}`);
      } else {
        if (paneIds.has(n.id)) problems.push(`duplicate pane id ${n.id}`);
        paneIds.add(n.id);
        const s = paneSession(n);
        if (s) {
          if (sessions.has(s)) problems.push(`session ${s} shown twice`);
          sessions.add(s);
        }
      }
    });
    if (tab.focused_pane && !findPane(tab.root, tab.focused_pane))
      problems.push(`${tab.id} focused pane missing`);
  }
  if (layout.active_tab && !findTab(layout, layout.active_tab)) problems.push('active tab missing');
  return problems;
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}
