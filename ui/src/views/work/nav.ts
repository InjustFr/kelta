// Opening panes from the work views: reuses an existing pane showing the same thing, else opens
// one per the requested placement. Every action that leaves a global view (Tickets, Now, Reviews)
// closes it and focuses the target in its project (FLOW §3.3, bug B1).

import type {
  PaneContent,
  Placement,
  ProjectId,
  ReviewKind,
  SessionInfo,
  WorkItem,
  WorkItemId,
} from '$lib/gen';
import { uiSetGlobalView, workResume } from '$lib/ipc/commands';
import {
  activateTab,
  allPanes,
  contentEquals,
  findContent,
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
const TICKETS_VIEW = 'kelta.view.tickets';
type TicketsContent = Extract<PaneContent, { kind: 'tickets' }>;
type TicketsState = Partial<Omit<TicketsContent, 'kind' | 'scope' | 'mode'>>;

function savedTickets(): TicketsState {
  try {
    return JSON.parse(localStorage.getItem(TICKETS_VIEW) ?? '{}') as TicketsState;
  } catch {
    return {};
  }
}

const ticketsContent = (s: TicketsState): PaneContent => ({
  kind: 'tickets',
  scope: ALL,
  view_id: null,
  who: 'mine',
  ...s,
  mode: 'list',
});

const REVIEWS_VIEW = 'kelta.view.reviews';

function savedReviewsTab(): ReviewKind | null {
  try {
    return (JSON.parse(localStorage.getItem(REVIEWS_VIEW) ?? '{}') as { tab?: ReviewKind }).tab ?? null;
  } catch {
    return null;
  }
}

/** The pane a global view shows; the Tickets view (list only) and Reviews seed their saved state. */
export function globalContent(view: GlobalView): PaneContent {
  if (view === 'now') return { kind: 'inbox' };
  if (view === 'reviews') return { kind: 'reviews', scope: ALL, tab: savedReviewsTab() };
  return ticketsContent(savedTickets());
}

/** Saves the Reviews view's tab and polls what it now shows (Team only while shown). */
export function saveReviewsView(tab: ReviewKind): void {
  try {
    localStorage.setItem(REVIEWS_VIEW, JSON.stringify({ tab }));
  } catch {
    // storage blocked: the tab lasts until the view closes
  }
  uiSetGlobalView({ content: globalContent('reviews') }).catch(() => {});
}

/**
 * Saves the Tickets view's who/group/sort/person/source and polls what it now shows.
 * shortcut: the contract has no ui_state command, so it lives in this webview's storage; add one if
 * it must follow Louis across machines.
 */
export function saveTicketsView(patch: TicketsState): void {
  const next = { ...savedTickets(), ...patch };
  try {
    localStorage.setItem(TICKETS_VIEW, JSON.stringify(next));
  } catch {
    // storage blocked: the state lasts until the view closes
  }
  uiSetGlobalView({ content: ticketsContent(next) }).catch(() => {});
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
