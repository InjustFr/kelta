// Opening panes from the work views: reuses an existing pane showing the same thing, else opens
// one per the requested placement. Every action that leaves a global view (Tickets, Now, Reviews)
// closes it and focuses the target in its project (FLOW §3.3, bug B1).

import type {
  Layout,
  PaneContent,
  PaneId,
  Placement,
  ProjectId,
  ReviewKind,
  SessionInfo,
  WorkItem,
  WorkItemId,
} from '$lib/gen';
import { workResume } from '$lib/ipc/commands';
import {
  activateTab,
  allPanes,
  contentEquals,
  findContent,
  findPane,
  focusPane,
  pane as makePane,
  replacePaneContent,
  splitPane,
  updateTab,
} from '$lib/layout';
import { GLOBAL, layout, projects, toasts, ui, work, type GlobalView } from '$lib/stores';

import { activateProject } from '../../shell/nav';

export interface OpenOptions {
  placement?: Placement;
  title?: string | null;
  workItemId?: WorkItemId | null;
  /** Reuse any pane matching this predicate instead of strict content equality. */
  match?: (content: PaneContent) => boolean;
  /** With `match`: show `content` in the reused pane (one Reviews tab per project). */
  replace?: boolean;
}

/**
 * Focuses the existing pane for `content` or opens a new one. Resolves false when no layout. From a
 * global view the project comes to the front first, in a new tab: never a hidden pane.
 */
export async function openContent(
  projectId: ProjectId,
  content: PaneContent,
  opts: OpenOptions = {},
): Promise<boolean> {
  if (ui.view !== null) {
    await activateProject(projectId);
    if (ui.view !== null) return false; // unknown project: stay in the view
    opts = { placement: 'new_tab', ...opts };
  }
  try {
    await layout.ensure(projectId);
  } catch {
    return false;
  }
  const current = layout.get(projectId);
  if (!current) return false;
  const match = opts.match ?? ((c: PaneContent) => contentEquals(c, content));
  const hit = findContent(current, match);
  if (hit) {
    layout.update(projectId, (l) => {
      const shown = opts.replace
        ? updateTab(l, hit.tabId, (t) => ({ ...t, root: replacePaneContent(t.root, hit.paneId, content) }))
        : l;
      return activateTab(focusPane(shown, hit.tabId, hit.paneId), hit.tabId);
    });
    return true;
  }
  layout.open(projectId, {
    content,
    placement: opts.placement ?? 'split_right',
    focus: true,
    tab_title: opts.title ?? null,
    work_item_id: opts.workItemId ?? null,
  });
  return true;
}

/**
 * Opens `content` from a pane of `projectId`. A global view's pane (`GLOBAL`) opens it in the item's
 * first project, else the active one, never in the sentinel.
 */
export async function openFrom(
  projectId: ProjectId,
  itemProjects: readonly ProjectId[],
  content: PaneContent,
): Promise<boolean> {
  const target = projectId === GLOBAL ? (itemProjects[0] ?? projects.activeId) : projectId;
  return target ? openContent(target, content) : false;
}

const ALL = { kind: 'all' } as const;
export const GLOBAL_VIEWS: readonly GlobalView[] = ['tickets', 'now', 'reviews'];
type TicketsContent = Extract<PaneContent, { kind: 'tickets' }>;
type TicketsState = Partial<Omit<TicketsContent, 'kind' | 'scope' | 'mode'>>;

/** Pre-#270 localStorage state (views' state, the place shown), read only to seed the `GLOBAL` layout. */
function stored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function legacy<T>(key: string): T | null {
  try {
    return JSON.parse(stored(key) ?? 'null') as T | null;
  } catch {
    return null;
  }
}

const ticketsContent = (s: TicketsState | null): PaneContent => ({
  kind: 'tickets',
  scope: ALL,
  view_id: null,
  who: 'mine',
  ...s,
  mode: 'list',
});

const reviewsContent = (tab: ReviewKind | null): PaneContent => ({ kind: 'reviews', scope: ALL, tab });

function seedContent(view: GlobalView): PaneContent {
  if (view === 'now') return { kind: 'inbox' };
  if (view === 'reviews')
    return reviewsContent(legacy<{ tab?: ReviewKind }>('kelta.view.reviews')?.tab ?? null);
  return ticketsContent(legacy<TicketsState>('kelta.view.tickets'));
}

/** The pane a global view shows: its pane in the `GLOBAL` layout; the Tickets view is list only. */
export function globalContent(view: GlobalView): PaneContent {
  const root = layout.get(GLOBAL)?.tabs.find((t) => t.id === view)?.root;
  const content = root?.type === 'pane' ? root.content : seedContent(view);
  return content.kind === 'tickets' ? { ...content, mode: 'list' } : content;
}

/**
 * The `GLOBAL` layout with one tab per view (tab id = pane id = view); a never-saved one takes the
 * pre-#270 place as its active tab. Unchanged when complete.
 */
export function seedGlobalLayout(l: Layout): Layout {
  const missing = GLOBAL_VIEWS.filter((v) => !l.tabs.some((t) => t.id === v));
  if (missing.length === 0) return l;
  const tabs = missing.map((v) => ({
    id: v,
    title: v,
    work_item_id: null,
    root: makePane(seedContent(v), v),
    focused_pane: v,
    zoomed_pane: null,
  }));
  const place = stored('kelta.place');
  const active = l.rev === 0 && GLOBAL_VIEWS.includes(place as GlobalView) ? place : l.active_tab;
  return { ...l, tabs: [...l.tabs, ...tabs], active_tab: active };
}

/** Loads the `GLOBAL` layout (seeded when incomplete). */
export async function loadGlobalLayout(): Promise<Layout | null> {
  await layout.ensure(GLOBAL);
  return layout.update(GLOBAL, seedGlobalLayout);
}

/**
 * Persists `patch` into pane `paneId`'s content in `projectId`'s layout (`GLOBAL` for a global
 * view). Merged onto the stored content: two changes in one tick must not drop the first.
 */
export function persistPaneContent(projectId: ProjectId, paneId: PaneId, patch: object): void {
  const current = layout.get(projectId);
  if (!current || !current.tabs.some((t) => findPane(t.root, paneId))) return;
  layout.update(projectId, (l) => ({
    ...l,
    tabs: l.tabs.map((t) => {
      const stored = findPane(t.root, paneId)?.pane.content;
      return stored
        ? { ...t, root: replacePaneContent(t.root, paneId, { ...stored, ...patch } as PaneContent) }
        : t;
    }),
  }));
}

/** Leaves Now and opens `content` in its project, in a new tab or the one already showing it. */
export async function openFromNow(
  projectId: ProjectId,
  content: PaneContent,
  opts: OpenOptions = {},
): Promise<void> {
  await activateProject(projectId);
  await openContent(projectId, content, { placement: 'new_tab', ...opts });
}

/** A review in the project's one Reviews tab (reused, never a split in the active tab). */
export function openReview(
  projectId: ProjectId,
  content: Extract<PaneContent, { kind: 'review_detail' }>,
): Promise<void> {
  return openFromNow(projectId, content, {
    title: 'Reviews',
    match: (c) => c.kind === 'review_detail',
    replace: true,
  });
}

/** Leaves Now and focuses the item's work tab; a closed tab is recreated by `work_resume`. */
export async function goToWork(item: WorkItem): Promise<boolean> {
  await activateProject(item.project_id);
  try {
    await layout.ensure(item.project_id);
    const tab = layout.get(item.project_id)?.tabs.find((t) => t.work_item_id === item.id);
    if (tab) {
      layout.update(item.project_id, (l) => activateTab(l, tab.id));
      return true;
    }
    work.upsert(await workResume({ id: item.id }));
    return false;
  } catch (err) {
    toasts.error(err, 'Open work item');
    return false;
  }
}

/**
 * Shows a session of a work item in its work tab, split down from the focused pane, focused and
 * zoomed (the review diff). Falls back to a new tab when the work tab is not there yet.
 */
export function showZoomedInWorkTab(item: WorkItem, session: SessionInfo): void {
  const pid = item.project_id;
  const tab = layout.get(pid)?.tabs.find((t) => t.work_item_id === item.id);
  const content: PaneContent = { kind: 'terminal', session_id: session.id };
  if (!tab) {
    layout.open(pid, {
      content,
      placement: 'new_tab',
      focus: true,
      tab_title: 'diff',
      work_item_id: item.id,
    });
    return;
  }
  const node = makePane(content);
  const from = tab.focused_pane ?? allPanes(tab.root)[0]?.id;
  if (!from) return;
  layout.update(pid, (l) =>
    activateTab(
      updateTab(l, tab.id, (t) => ({
        ...t,
        root: splitPane(t.root, from, 'column', node),
        focused_pane: node.id,
        zoomed_pane: node.id,
      })),
      tab.id,
    ),
  );
}
