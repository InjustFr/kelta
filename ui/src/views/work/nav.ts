// Opening panes from the work views: reuses an existing pane showing the same thing, else opens
// one per the requested placement. Every action that leaves Now closes it and focuses the target
// in its project (FLOW §3.3, bug B1).

import type { PaneContent, Placement, ProjectId, SessionInfo, WorkItem, WorkItemId } from '$lib/gen';
import { workResume } from '$lib/ipc/commands';
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
import { layout, toasts, work } from '$lib/stores';

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

/** Focuses the existing pane for `content` or opens a new one. Resolves false when no layout. */
export async function openContent(
  projectId: ProjectId,
  content: PaneContent,
  opts: OpenOptions = {},
): Promise<boolean> {
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
